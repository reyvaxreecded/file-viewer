use crate::{
    App,
    error::{Error, Result},
    now,
};
use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use futures_util::StreamExt;
use glassftp_sftp::{Attrs, BLOCK, Flags};
use glassftp_shared::{Upload, UploadRequest};
use uuid::Uuid;
pub const CHUNK_LIMIT: usize = 8 * 1024 * 1024;
pub fn id(raw: &str) -> Result<Uuid> {
    Uuid::parse_str(raw).map_err(|_| Error::bad("Identifiant invalide"))
}
pub async fn list(State(a): State<App>) -> Result<Json<Vec<Upload>>> {
    Ok(Json(
        a.store
            .list::<Upload>("uploads")
            .await?
            .into_iter()
            .filter(|u| u.status != "complete")
            .collect(),
    ))
}
pub async fn create(
    State(a): State<App>,
    Json(r): Json<UploadRequest>,
) -> Result<(StatusCode, Json<Upload>)> {
    if r.path.is_root() || r.size > a.config.max_upload {
        return Err(Error::bad("Destination ou taille invalide"));
    }
    let _guard = a.mutation.lock().await;
    let active: Vec<Upload> = a.store.list("uploads").await?;
    if active.iter().filter(|u| u.status != "complete").count() >= 100 {
        return Err(Error::bad(
            "100 transferts en attente : annulez les anciens transferts",
        ));
    }
    if active
        .iter()
        .any(|u| u.path == r.path && u.status != "complete")
    {
        return Err(Error::conflict("Un transfert réserve déjà ce chemin"));
    }
    let c = a.pool.get().await?;
    let target = c.resolve(&r.path, true).await?;
    c.absent(&target).await?;
    let uuid = Uuid::new_v4();
    let u = Upload {
        id: uuid.to_string(),
        path: r.path,
        size: r.size,
        offset: 0,
        status: "active".into(),
        created: now(),
        updated: now(),
    };
    // Journal first. A crash can leave an empty reservation, never an exposed partial file.
    a.store.put("uploads", &u.id, &u).await?;
    c.create_empty(&c.internal("uploads", uuid)).await?;
    c.recycle();
    Ok((StatusCode::CREATED, Json(u)))
}
pub async fn status(State(a): State<App>, Path(raw): Path<String>) -> Result<Json<Upload>> {
    id(&raw)?;
    Ok(Json(a.store.get("uploads", &raw).await?))
}
pub async fn chunk(
    State(a): State<App>,
    Path(raw): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<Upload>> {
    let uuid = id(&raw)?;
    let offset: u64 = headers
        .get("upload-offset")
        .and_then(|s| s.to_str().ok())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| Error::bad("Upload-Offset requis"))?;
    let len: u64 = headers
        .get("content-length")
        .and_then(|s| s.to_str().ok())
        .and_then(|s| s.parse().ok())
        .filter(|n| *n > 0 && *n <= CHUNK_LIMIT as u64)
        .ok_or_else(|| Error::bad("Content-Length requis, fragment de 1 à 8 Mio"))?;
    let lock = a.transfer_lock(&raw)?;
    let _guard = lock.lock().await;
    let mut u: Upload = a.store.get("uploads", &raw).await?;
    if u.status != "active" || offset != u.offset {
        return Err(Error::conflict(
            "Position ou état du transfert modifié ; relisez son état",
        ));
    }
    if offset.checked_add(len).is_none_or(|end| end > u.size) {
        return Err(Error::bad("Fragment hors limites"));
    }
    let _permit = a.transfers.clone().try_acquire_owned().map_err(|_| {
        Error(
            StatusCode::TOO_MANY_REQUESTS,
            "busy",
            "Transferts occupés ; réessayez".into(),
        )
    })?;
    let c = a.pool.get().await?;
    let h = c
        .raw
        .open(c.internal("uploads", uuid), Flags::WRITE, Attrs::default())
        .await?
        .handle;
    let mut stream = body.into_data_stream();
    let mut written = 0u64;
    while let Some(frame) = tokio::time::timeout(std::time::Duration::from_secs(60), stream.next())
        .await
        .map_err(|_| Error::bad("Fragment interrompu"))?
    {
        let frame = frame.map_err(|_| Error::bad("Corps du fragment interrompu"))?;
        if written + frame.len() as u64 > len {
            return Err(Error::bad("Fragment trop long"));
        }
        for b in frame.chunks(BLOCK) {
            c.raw.write(h.clone(), offset + written, b.to_vec()).await?;
            written += b.len() as u64;
        }
    }
    if written != len {
        return Err(Error::bad("Fragment incomplet"));
    }
    // Remove any uncommitted tail left by a disconnected earlier attempt.
    c.raw
        .fsetstat(
            h.clone(),
            Attrs {
                size: Some(offset + written),
                ..Default::default()
            },
        )
        .await?;
    c.raw.fsync(h.clone()).await?;
    c.raw.close(h).await?;
    u.offset += written;
    u.updated = now();
    a.store.put("uploads", &raw, &u).await?;
    c.recycle();
    Ok(Json(u))
}
pub async fn complete(State(a): State<App>, Path(raw): Path<String>) -> Result<Json<Upload>> {
    // Finish the journal even if the browser closes while the rename is in flight.
    tokio::spawn(async move {
        let uuid = id(&raw)?;
        let lock = a.transfer_lock(&raw)?;
        let _guard = lock.lock().await;
        let _mutation = a.mutation.lock().await;
        let mut u: Upload = a.store.get("uploads", &raw).await?;
        if u.status == "complete" {
            return Ok(Json(u));
        }
        if u.status != "active" || u.offset != u.size {
            return Err(Error::conflict(
                "Transfert incomplet ou finalisation à vérifier",
            ));
        }
        let c = a.pool.get().await?;
        let temp = c.internal("uploads", uuid);
        if c.raw.lstat(temp.clone()).await?.attrs.size != Some(u.size) {
            return Err(Error::conflict("Taille distante incohérente"));
        }
        let dest = c.resolve(&u.path, true).await?;
        c.absent(&dest).await?;
        u.status = "committing".into();
        a.store.put("uploads", &raw, &u).await?;
        // Standard SFTP v3 rename MUST NOT overwrite an existing destination.
        c.raw.rename(temp, dest).await?;
        u.status = "complete".into();
        a.store.put("uploads", &raw, &u).await?;
        a.changed(u.path.parent().as_str());
        c.recycle();
        Ok(Json(u))
    })
    .await
    .map_err(|_| Error::internal())?
}
pub async fn cancel(State(a): State<App>, Path(raw): Path<String>) -> Result<StatusCode> {
    let uuid = id(&raw)?;
    let lock = a.transfer_lock(&raw)?;
    let _guard = lock.lock().await;
    let u: Upload = a.store.get("uploads", &raw).await?;
    if u.status == "committing" {
        return Err(Error::conflict(
            "Finalisation incertaine : conserver le journal et vérifier côté serveur",
        ));
    }
    if u.status != "complete" {
        let c = a.pool.get().await?;
        match c.raw.remove(c.internal("uploads", uuid)).await {
            Ok(_) => (),
            Err(e) if glassftp_sftp::missing(&e) => (),
            Err(e) => return Err(e.into()),
        }
        c.recycle();
    }
    a.store.delete("uploads", &raw).await?;
    Ok(StatusCode::NO_CONTENT)
}
