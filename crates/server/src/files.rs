use crate::{
    App,
    error::{Error, Result},
    now,
};
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use futures_util::StreamExt;
use glassftp_sftp::{Attrs, BLOCK, Flags};
use glassftp_shared::*;
use serde::Deserialize;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct Browse {
    #[serde(default = "VPath::root")]
    path: VPath,
    #[serde(default)]
    offset: usize,
    #[serde(default = "page_size")]
    limit: usize,
    #[serde(default)]
    q: String,
}
fn page_size() -> usize {
    200
}
#[derive(Deserialize)]
pub struct Download {
    path: VPath,
    #[serde(default)]
    inline: bool,
}
pub async fn health(State(a): State<App>) -> Result<Json<serde_json::Value>> {
    let c = a.pool.get().await?;
    c.info(&VPath::root()).await?;
    c.recycle();
    Ok(Json(serde_json::json!({"status":"ok","storage":"sftp"})))
}
pub async fn list(State(a): State<App>, Query(q): Query<Browse>) -> Result<Json<Listing>> {
    if q.limit == 0 || q.limit > 500 || q.q.len() > 200 {
        return Err(Error::bad("Pagination ou recherche invalide"));
    }
    let c = a.pool.get().await?;
    let mut entries = c.list(&q.path).await?;
    c.recycle();
    if !q.q.is_empty() {
        let needle = q.q.to_lowercase();
        entries.retain(|e| e.name.to_lowercase().contains(&needle));
    }
    let total = entries.len();
    Ok(Json(Listing {
        path: q.path,
        entries: entries.into_iter().skip(q.offset).take(q.limit).collect(),
        total,
        offset: q.offset,
        limit: q.limit,
    }))
}
pub async fn info(State(a): State<App>, Query(q): Query<PathRequest>) -> Result<Json<Entry>> {
    let c = a.pool.get().await?;
    let e = c.info(&q.path).await?;
    c.recycle();
    Ok(Json(e))
}
pub async fn mkdir(State(a): State<App>, Json(q): Json<PathRequest>) -> Result<StatusCode> {
    if q.path.is_root() {
        return Err(Error::bad("Racine protégée"));
    }
    let _guard = a.mutation.lock().await;
    let c = a.pool.get().await?;
    let p = c.resolve(&q.path, true).await?;
    c.absent(&p).await?;
    c.raw
        .mkdir(
            p,
            Attrs {
                permissions: Some(0o750),
                ..Default::default()
            },
        )
        .await?;
    a.changed(q.path.parent().as_str());
    c.recycle();
    Ok(StatusCode::CREATED)
}
fn nonroot(path: &VPath) -> Result<()> {
    if path.is_root() {
        Err(Error::bad("Racine protégée"))
    } else {
        Ok(())
    }
}
fn version(e: &Entry, expected: &str) -> Result<()> {
    if e.version != expected {
        Err(Error::conflict(
            "Cet élément a été modifié ; actualisez le dossier",
        ))
    } else {
        Ok(())
    }
}
pub async fn move_file(State(a): State<App>, Json(q): Json<MoveRequest>) -> Result<StatusCode> {
    nonroot(&q.source)?;
    nonroot(&q.destination)?;
    if q.destination
        .as_str()
        .starts_with(&format!("{}/", q.source.as_str()))
    {
        return Err(Error::bad(
            "Un dossier ne peut pas être déplacé dans lui-même",
        ));
    }
    let _guard = a.mutation.lock().await;
    let c = a.pool.get().await?;
    version(&c.info(&q.source).await?, &q.version)?;
    let from = c.resolve(&q.source, false).await?;
    let to = c.resolve(&q.destination, true).await?;
    c.absent(&to).await?;
    c.raw.rename(from, to).await?;
    a.changed(q.source.parent().as_str());
    a.changed(q.destination.parent().as_str());
    c.recycle();
    Ok(StatusCode::NO_CONTENT)
}
pub async fn copy(State(a): State<App>, Json(q): Json<MoveRequest>) -> Result<StatusCode> {
    nonroot(&q.source)?;
    nonroot(&q.destination)?;
    let _guard = a.mutation.lock().await;
    let _permit = a
        .transfers
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error::conflict("Transferts occupés"))?;
    let c = a.pool.get().await?;
    let original = c.info(&q.source).await?;
    version(&original, &q.version)?;
    if original.kind != FileKind::File {
        return Err(Error::bad(
            "La copie récursive des dossiers n’est pas encore disponible",
        ));
    }
    let dest = c.resolve(&q.destination, true).await?;
    c.absent(&dest).await?;
    let temp = c.internal("uploads", Uuid::new_v4());
    let input = c
        .raw
        .open(
            c.resolve(&q.source, false).await?,
            Flags::READ,
            Attrs::default(),
        )
        .await?
        .handle;
    let output = c
        .raw
        .open(
            temp.clone(),
            Flags::WRITE | Flags::CREATE | Flags::EXCLUDE,
            Attrs {
                permissions: Some(0o600),
                ..Default::default()
            },
        )
        .await?
        .handle;
    let work = async {
        let mut offset = 0;
        while offset < original.size {
            let d = c
                .raw
                .read(
                    input.clone(),
                    offset,
                    (original.size - offset).min(BLOCK as u64) as u32,
                )
                .await?;
            if d.data.is_empty() {
                return Err(Error::conflict("Source interrompue"));
            }
            let len = d.data.len() as u64;
            c.raw.write(output.clone(), offset, d.data).await?;
            offset += len;
        }
        c.raw.fsync(output.clone()).await?;
        c.raw.close(input).await?;
        c.raw.close(output).await?;
        version(&c.info(&q.source).await?, &q.version)?;
        c.absent(&dest).await?;
        c.raw.rename(temp.clone(), dest).await?;
        Ok::<_, Error>(())
    }
    .await;
    if work.is_err() {
        let _ = c.raw.remove(temp).await;
    }
    work?;
    a.changed(q.destination.parent().as_str());
    c.recycle();
    Ok(StatusCode::CREATED)
}
pub async fn trash(State(a): State<App>, Json(q): Json<VersionRequest>) -> Result<Json<TrashItem>> {
    nonroot(&q.path)?;
    let _guard = a.mutation.lock().await;
    let c = a.pool.get().await?;
    let e = c.info(&q.path).await?;
    version(&e, &q.version)?;
    let uuid = Uuid::new_v4();
    let mut item = TrashItem {
        id: uuid.to_string(),
        original: q.path.clone(),
        deleted: now(),
        entry: e,
        status: "pending".into(),
    };
    a.store.put("trash", &item.id, &item).await?;
    c.raw
        .rename(c.resolve(&q.path, false).await?, c.internal("trash", uuid))
        .await?;
    item.status = "ready".into();
    a.store.put("trash", &item.id, &item).await?;
    a.changed(q.path.parent().as_str());
    c.recycle();
    Ok(Json(item))
}
pub async fn trash_list(State(a): State<App>) -> Result<Json<Vec<TrashItem>>> {
    Ok(Json(a.store.list("trash").await?))
}
pub async fn restore(
    State(a): State<App>,
    Path(raw): Path<String>,
    Json(q): Json<RestoreRequest>,
) -> Result<StatusCode> {
    nonroot(&q.destination)?;
    let uuid = crate::uploads::id(&raw)?;
    let _guard = a.mutation.lock().await;
    let _: TrashItem = a.store.get("trash", &raw).await?;
    let c = a.pool.get().await?;
    let to = c.resolve(&q.destination, true).await?;
    c.absent(&to).await?;
    c.raw.rename(c.internal("trash", uuid), to).await?;
    a.store.delete("trash", &raw).await?;
    a.changed(q.destination.parent().as_str());
    c.recycle();
    Ok(StatusCode::NO_CONTENT)
}
pub async fn purge(State(a): State<App>, Path(raw): Path<String>) -> Result<StatusCode> {
    let uuid = crate::uploads::id(&raw)?;
    let _guard = a.mutation.lock().await;
    let _: TrashItem = a.store.get("trash", &raw).await?;
    let c = a.pool.get().await?;
    let mut stack = vec![(c.internal("trash", uuid), false)];
    let mut count = 0;
    while let Some((p, visited)) = stack.pop() {
        count += 1;
        if count > 20000 {
            return Err(Error::bad(
                "Suppression trop volumineuse : intervention administrateur requise",
            ));
        }
        let attrs = match c.raw.lstat(p.clone()).await {
            Ok(s) => s.attrs,
            Err(e) if glassftp_sftp::missing(&e) => continue,
            Err(e) => return Err(e.into()),
        };
        if attrs.file_type().is_dir() {
            if visited {
                c.raw.rmdir(p).await?;
                continue;
            }
            stack.push((p.clone(), true));
            let h = c.raw.opendir(p.clone()).await?.handle;
            loop {
                match c.raw.readdir(h.clone()).await {
                    Ok(page) => {
                        for f in page.files {
                            if f.filename == "." || f.filename == ".." {
                                continue;
                            }
                            if f.filename.contains('/') || f.filename.contains('\\') {
                                return Err(Error::bad("Nom distant invalide"));
                            }
                            stack.push((format!("{p}/{}", f.filename), false));
                            if stack.len() > 20000 {
                                return Err(Error::bad("Corbeille trop volumineuse"));
                            }
                        }
                    }
                    Err(e) if glassftp_sftp::eof(&e) => break,
                    Err(e) => return Err(e.into()),
                }
            }
            c.raw.close(h).await?;
        } else {
            c.raw.remove(p).await?;
        }
    }
    a.store.delete("trash", &raw).await?;
    a.changed("/trash");
    c.recycle();
    Ok(StatusCode::NO_CONTENT)
}
pub async fn text(State(a): State<App>, Query(q): Query<PathRequest>) -> Result<Json<TextFile>> {
    let c = a.pool.get().await?;
    let (content, version) = c.read_text(&q.path).await?;
    c.recycle();
    Ok(Json(TextFile {
        path: q.path,
        content,
        version,
    }))
}
pub async fn save_text(State(a): State<App>, Json(q): Json<SaveText>) -> Result<Json<TextFile>> {
    if q.content.len() > glassftp_sftp::TEXT_LIMIT || q.content.contains('\0') {
        return Err(Error::bad("Texte UTF-8 limité à 1 Mio"));
    }
    let _guard = a.mutation.lock().await;
    let c = a.pool.get().await?;
    let (_, hash) = c.read_text(&q.path).await?;
    if hash != q.version {
        return Err(Error::conflict(
            "Le fichier a changé. Rechargez-le et fusionnez vos modifications avant de sauvegarder.",
        ));
    }
    let dest = c.resolve(&q.path, false).await?;
    let original_attrs = c.raw.lstat(dest.clone()).await?.attrs;
    let temp = c.internal("uploads", Uuid::new_v4());
    c.write_all(&temp, q.content.as_bytes()).await?;
    let work = async {
        let (_, current) = c.read_text(&q.path).await?;
        if current != q.version {
            return Err(Error::conflict("Modification concurrente détectée"));
        }
        c.raw
            .setstat(
                temp.clone(),
                Attrs {
                    permissions: original_attrs.permissions.map(|m| m & 0o777),
                    ..Default::default()
                },
            )
            .await?;
        c.replace(&temp, &dest).await?;
        Ok::<_, Error>(())
    }
    .await;
    if work.is_err() {
        let _ = c.raw.remove(temp).await;
    }
    work?;
    use sha2::{Digest, Sha256};
    let hash = format!("{:x}", Sha256::digest(q.content.as_bytes()));
    a.changed(q.path.parent().as_str());
    c.recycle();
    Ok(Json(TextFile {
        path: q.path,
        content: q.content,
        version: hash,
    }))
}
pub async fn download(
    State(a): State<App>,
    Query(q): Query<Download>,
    headers: HeaderMap,
) -> Result<Response> {
    let permit = a.transfers.clone().try_acquire_owned().map_err(|_| {
        Error(
            StatusCode::TOO_MANY_REQUESTS,
            "busy",
            "Transferts occupés ; réessayez".into(),
        )
    })?;
    let c = a.pool.get().await?;
    let e = c.info(&q.path).await?;
    if e.kind != FileKind::File {
        return Err(Error::bad("Sélectionnez un fichier"));
    }
    let h = c
        .raw
        .open(
            c.resolve(&q.path, false).await?,
            Flags::READ,
            Attrs::default(),
        )
        .await?
        .handle;
    // Descriptor metadata is the authority if a rename raced the open.
    let size = c
        .raw
        .fstat(h.clone())
        .await?
        .attrs
        .size
        .ok_or_else(Error::internal)?;
    let etag = format!("W/\"{}\"", e.version);
    let range_header = headers.get(header::RANGE).and_then(|h| h.to_str().ok());
    // Our metadata ETag is weak: If-Range cannot safely validate it; serve the complete representation.
    let range_header = if headers.contains_key(header::IF_RANGE) {
        None
    } else {
        range_header
    };
    let (start, len, status) = if let Some(value) = range_header {
        match crate::range::parse(value, size) {
            Ok((s, end)) => (s, end - s + 1, StatusCode::PARTIAL_CONTENT),
            Err(_) => {
                c.raw.close(h).await?;
                c.recycle();
                return Ok((
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    [(header::CONTENT_RANGE, format!("bytes */{size}"))],
                )
                    .into_response());
            }
        }
    } else {
        (0, size, StatusCode::OK)
    };
    let (tx, rx) =
        tokio::sync::mpsc::channel::<std::result::Result<bytes::Bytes, std::io::Error>>(2);
    tokio::spawn(async move {
        let _permit = permit;
        let mut offset = start;
        let work = async {
            while offset < start + len {
                if tx.is_closed() {
                    return Err(glassftp_sftp::Error::Unavailable);
                }
                let data = c
                    .raw
                    .read(
                        h.clone(),
                        offset,
                        (start + len - offset).min(BLOCK as u64) as u32,
                    )
                    .await?
                    .data;
                if data.is_empty() {
                    return Err(glassftp_sftp::Error::Protocol);
                }
                offset += data.len() as u64;
                tokio::time::timeout(
                    std::time::Duration::from_secs(60),
                    tx.send(Ok(bytes::Bytes::from(data))),
                )
                .await
                .map_err(|_| glassftp_sftp::Error::Unavailable)?
                .map_err(|_| glassftp_sftp::Error::Unavailable)?;
            }
            c.raw.close(h).await?;
            Ok::<_, glassftp_sftp::Error>(())
        }
        .await;
        match work {
            Ok(()) => c.recycle(),
            Err(err) => {
                let _ = tx.try_send(Err(std::io::Error::other(err.to_string())));
            }
        }
    });
    let safe_inline = q.inline
        && matches!(
            e.mime.as_str(),
            "image/png"
                | "image/jpeg"
                | "image/gif"
                | "image/webp"
                | "application/pdf"
                | "video/mp4"
                | "video/webm"
                | "audio/mpeg"
                | "audio/ogg"
                | "audio/wav"
        );
    let name = percent_encoding::utf8_percent_encode(&e.name, percent_encoding::NON_ALPHANUMERIC);
    let mut response = Body::from_stream(ReceiverStream::new(rx)).into_response();
    *response.status_mut() = status;
    let rh = response.headers_mut();
    rh.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&len.to_string()).map_err(|_| Error::internal())?,
    );
    rh.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    rh.insert(
        header::ETAG,
        HeaderValue::from_str(&etag).map_err(|_| Error::internal())?,
    );
    rh.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(if safe_inline {
            &e.mime
        } else {
            "application/octet-stream"
        })
        .map_err(|_| Error::internal())?,
    );
    rh.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "{}; filename*=UTF-8''{name}",
            if safe_inline { "inline" } else { "attachment" }
        ))
        .map_err(|_| Error::internal())?,
    );
    rh.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'; frame-ancestors 'self'"),
    );
    if status == StatusCode::PARTIAL_CONTENT {
        rh.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{}/{size}", start + len - 1))
                .map_err(|_| Error::internal())?,
        );
    }
    Ok(response)
}
pub async fn events(
    State(a): State<App>,
) -> Sse<impl futures_util::Stream<Item = std::result::Result<Event, std::convert::Infallible>>> {
    let stream = BroadcastStream::new(a.events.subscribe()).map(|result| {
        Ok(Event::default()
            .event("change")
            .data(result.unwrap_or_else(|_| "{\"path\":\"*\"}".into())))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
