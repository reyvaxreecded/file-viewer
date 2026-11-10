//! All shared-file I/O crosses SFTP. No local filesystem fallback exists.
use glassftp_shared::{Entry, FileKind, VPath};
use russh::{
    client,
    keys::{HashAlg, PublicKeyOrCertificate},
};
use russh_sftp::{
    client::RawSftpSession,
    protocol::{FileAttributes, OpenFlags, Packet, StatusCode},
};
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub const BLOCK: usize = 32 * 1024;
pub const TEXT_LIMIT: usize = 1024 * 1024;
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Fichier ou dossier introuvable")]
    NotFound,
    #[error("Accès refusé ou lien symbolique interdit")]
    Forbidden,
    #[error("Un élément existe déjà ou a été modifié")]
    Conflict,
    #[error("Limite de taille ou de ressources atteinte")]
    Limit,
    #[error("Connexion SFTP indisponible ou identité SSH non reconnue")]
    Unavailable,
    #[error("Opération SFTP non prise en charge")]
    Unsupported,
    #[error("Réponse SFTP invalide")]
    Protocol,
}
impl From<russh_sftp::client::error::Error> for Error {
    fn from(e: russh_sftp::client::error::Error) -> Self {
        use russh_sftp::client::error::Error as E;
        match e {
            E::Status(s) => match s.status_code {
                StatusCode::NoSuchFile => Self::NotFound,
                StatusCode::PermissionDenied => Self::Forbidden,
                StatusCode::OpUnsupported => Self::Unsupported,
                _ => Self::Protocol,
            },
            E::Timeout | E::IO(_) => Self::Unavailable,
            _ => Self::Protocol,
        }
    }
}
#[derive(Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub root: String,
    pub host_key_sha256: String,
    pub connections: usize,
    pub timeout: Duration,
}
struct HostVerifier(String);
impl client::Handler for HostVerifier {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        Ok(key.certificate().is_none()
            && key.public_key().fingerprint(HashAlg::Sha256).to_string() == self.0)
    }
}
pub struct Connection {
    pub raw: RawSftpSession,
    ssh: client::Handle<HostVerifier>,
    root: String,
    posix_rename: bool,
}
impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.raw.close_session();
    }
}
#[derive(Clone)]
pub struct Pool(Arc<PoolInner>);
struct PoolInner {
    config: Config,
    idle: Mutex<Vec<Connection>>,
    permits: Arc<Semaphore>,
}
pub struct Lease {
    conn: Option<Connection>,
    pool: Pool,
    _permit: OwnedSemaphorePermit,
}
impl std::ops::Deref for Lease {
    type Target = Connection;
    fn deref(&self) -> &Self::Target {
        self.conn
            .as_ref()
            .expect("lease owns connection until recycle")
    }
}
impl Lease {
    /// Only successful operations recycle their session. Dropped/cancelled work closes it.
    pub fn recycle(mut self) {
        if let (Some(c), Ok(mut idle)) = (self.conn.take(), self.pool.0.idle.lock()) {
            if !c.ssh.is_closed() {
                idle.push(c);
            }
        }
    }
}
impl Pool {
    pub fn new(config: Config) -> Self {
        let n = config.connections.max(2);
        Self(Arc::new(PoolInner {
            config,
            idle: Mutex::new(Vec::new()),
            permits: Arc::new(Semaphore::new(n)),
        }))
    }
    pub async fn get(&self) -> Result<Lease> {
        let permit = tokio::time::timeout(
            self.0.config.timeout,
            self.0.permits.clone().acquire_owned(),
        )
        .await
        .map_err(|_| Error::Unavailable)?
        .map_err(|_| Error::Unavailable)?;
        let mut cached = self.0.idle.lock().map_err(|_| Error::Unavailable)?.pop();
        if cached.as_ref().is_some_and(|c| c.ssh.is_closed()) {
            cached = None;
        }
        let conn = match cached {
            Some(c) => c,
            None => tokio::time::timeout(self.0.config.timeout, connect(&self.0.config))
                .await
                .map_err(|_| Error::Unavailable)??,
        };
        conn.prepare().await?;
        Ok(Lease {
            conn: Some(conn),
            pool: self.clone(),
            _permit: permit,
        })
    }
}
async fn connect(c: &Config) -> Result<Connection> {
    let config = client::Config {
        inactivity_timeout: Some(Duration::from_secs(120)),
        keepalive_interval: Some(Duration::from_secs(20)),
        ..Default::default()
    };
    let mut ssh = client::connect(
        Arc::new(config),
        (c.host.as_str(), c.port),
        HostVerifier(c.host_key_sha256.clone()),
    )
    .await
    .map_err(|_| Error::Unavailable)?;
    if !ssh
        .authenticate_password(&c.username, &c.password)
        .await
        .map_err(|_| Error::Unavailable)?
        .success()
    {
        return Err(Error::Unavailable);
    }
    let channel = ssh
        .channel_open_session()
        .await
        .map_err(|_| Error::Unavailable)?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|_| Error::Unavailable)?;
    let raw = RawSftpSession::new_with_config(
        channel.into_stream(),
        russh_sftp::client::Config {
            max_packet_len: 262144,
            max_concurrent_reads: 1,
            max_concurrent_writes: 1,
            request_timeout_secs: c.timeout.as_secs(),
            ..Default::default()
        },
    );
    let version = raw.init().await?;
    // Durability is required: do not silently downgrade fsync.
    if !version.extensions.contains_key("fsync@openssh.com") {
        return Err(Error::Unsupported);
    }
    let root = raw
        .realpath(c.root.clone())
        .await?
        .files
        .into_iter()
        .next()
        .ok_or(Error::Protocol)?
        .filename;
    if root != c.root.trim_end_matches('/') && !(root == "/" && c.root == "/") {
        return Err(Error::Forbidden);
    }
    let conn = Connection {
        raw,
        ssh,
        root,
        posix_rename: version.extensions.contains_key("posix-rename@openssh.com"),
    };
    if !conn
        .raw
        .lstat(conn.root.clone())
        .await?
        .attrs
        .file_type()
        .is_dir()
    {
        return Err(Error::Forbidden);
    }
    Ok(conn)
}
impl Connection {
    pub fn remote(&self, p: &VPath) -> String {
        format!("{}{}", self.root.trim_end_matches('/'), p.as_str())
    }
    pub fn internal(&self, area: &str, id: uuid::Uuid) -> String {
        format!("{}/.glassftp/{area}/{id}", self.root.trim_end_matches('/'))
    }
    pub async fn prepare(&self) -> Result<()> {
        for suffix in [".glassftp", ".glassftp/uploads", ".glassftp/trash"] {
            let p = format!("{}/{suffix}", self.root.trim_end_matches('/'));
            match self.raw.lstat(p.clone()).await {
                Ok(a) if a.attrs.file_type().is_dir() => (),
                Ok(_) => return Err(Error::Forbidden),
                Err(e) if missing(&e) => {
                    self.raw
                        .mkdir(
                            p,
                            FileAttributes {
                                permissions: Some(0o700),
                                ..Default::default()
                            },
                        )
                        .await?;
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
    /// Reject every symbolic link and special file, including intermediate components.
    /// OpenSSH chroot remains necessary to close the protocol's lstat/open race.
    pub async fn resolve(&self, path: &VPath, missing_leaf: bool) -> Result<String> {
        let mut remote = self.root.trim_end_matches('/').to_string();
        let parts: Vec<_> = path.as_str().split('/').filter(|s| !s.is_empty()).collect();
        for (i, part) in parts.iter().enumerate() {
            remote.push('/');
            remote.push_str(part);
            match self.raw.lstat(remote.clone()).await {
                Ok(a) => {
                    let ty = a.attrs.file_type();
                    if ty.is_symlink()
                        || (!ty.is_file() && !ty.is_dir())
                        || (i + 1 < parts.len() && !ty.is_dir())
                    {
                        return Err(Error::Forbidden);
                    }
                }
                Err(e) if missing_leaf && i + 1 == parts.len() && missing(&e) => (),
                Err(e) => return Err(e.into()),
            }
        }
        if remote.is_empty() {
            remote.push('/');
        }
        Ok(remote)
    }
    pub async fn absent(&self, remote: &str) -> Result<()> {
        match self.raw.lstat(remote).await {
            Ok(_) => Err(Error::Conflict),
            Err(e) if missing(&e) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
    pub async fn info(&self, path: &VPath) -> Result<Entry> {
        let p = self.resolve(path, false).await?;
        entry(path.clone(), self.raw.lstat(p).await?.attrs)
    }
    pub async fn list(&self, path: &VPath) -> Result<Vec<Entry>> {
        let p = self.resolve(path, false).await?;
        let h = self.raw.opendir(p).await?.handle;
        let mut entries = Vec::new();
        let result = async {
            loop {
                match self.raw.readdir(h.clone()).await {
                    Ok(page) => {
                        for f in page.files {
                            if let Ok(p) = path.join(&f.filename) {
                                if let Ok(e) = entry(p, f.attrs) {
                                    entries.push(e);
                                }
                            }
                            if entries.len() > 20_000 {
                                return Err(Error::Limit);
                            }
                        }
                    }
                    Err(e) if eof(&e) => break,
                    Err(e) => return Err(e.into()),
                }
            }
            entries.sort_by(|a, b| {
                (a.kind != FileKind::Directory, a.name.to_lowercase())
                    .cmp(&(b.kind != FileKind::Directory, b.name.to_lowercase()))
            });
            Ok(entries)
        }
        .await;
        let close = self.raw.close(h).await;
        let result = result?;
        close?;
        Ok(result)
    }
    pub async fn create_empty(&self, remote: &str) -> Result<()> {
        let h = self
            .raw
            .open(
                remote,
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                FileAttributes {
                    permissions: Some(0o600),
                    ..Default::default()
                },
            )
            .await?
            .handle;
        self.raw.fsync(h.clone()).await?;
        self.raw.close(h).await?;
        Ok(())
    }
    pub async fn read_text(&self, p: &VPath) -> Result<(String, String)> {
        let e = self.info(p).await?;
        if e.kind != FileKind::File || e.size > TEXT_LIMIT as u64 {
            return Err(Error::Limit);
        }
        let h = self
            .raw
            .open(
                self.resolve(p, false).await?,
                OpenFlags::READ,
                FileAttributes::default(),
            )
            .await?
            .handle;
        let mut bytes = Vec::new();
        let result = async {
            loop {
                match self
                    .raw
                    .read(h.clone(), bytes.len() as u64, BLOCK as u32)
                    .await
                {
                    Ok(d) => {
                        if d.data.is_empty() {
                            break;
                        }
                        bytes.extend(d.data);
                        if bytes.len() > TEXT_LIMIT {
                            return Err(Error::Limit);
                        }
                    }
                    Err(e) if eof(&e) => break,
                    Err(e) => return Err(e.into()),
                }
            }
            if bytes.contains(&0) {
                return Err(Error::Unsupported);
            }
            let hash = format!("{:x}", Sha256::digest(&bytes));
            Ok((
                String::from_utf8(bytes).map_err(|_| Error::Unsupported)?,
                hash,
            ))
        }
        .await;
        let close = self.raw.close(h).await;
        let result = result?;
        close?;
        Ok(result)
    }
    pub async fn write_all(&self, remote: &str, data: &[u8]) -> Result<()> {
        let h = self
            .raw
            .open(
                remote,
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                FileAttributes {
                    permissions: Some(0o600),
                    ..Default::default()
                },
            )
            .await?
            .handle;
        let result = async {
            for (i, b) in data.chunks(BLOCK).enumerate() {
                self.raw
                    .write(h.clone(), (i * BLOCK) as u64, b.to_vec())
                    .await?;
            }
            self.raw.fsync(h.clone()).await?;
            Ok::<_, Error>(())
        }
        .await;
        let close = self.raw.close(h).await;
        result?;
        close?;
        Ok(())
    }
    pub async fn replace(&self, from: &str, to: &str) -> Result<()> {
        if !self.posix_rename {
            return Err(Error::Unsupported);
        }
        let mut data = Vec::new();
        for s in [from, to] {
            data.extend_from_slice(&(s.len() as u32).to_be_bytes());
            data.extend_from_slice(s.as_bytes());
        }
        match self.raw.extended("posix-rename@openssh.com", data).await? {
            Packet::Status(s) if s.status_code == StatusCode::Ok => Ok(()),
            Packet::Status(s) => Err(russh_sftp::client::error::Error::Status(s).into()),
            _ => Err(Error::Protocol),
        }
    }
}
pub fn missing(e: &russh_sftp::client::error::Error) -> bool {
    matches!(e, russh_sftp::client::error::Error::Status(s) if s.status_code == StatusCode::NoSuchFile)
}
pub fn eof(e: &russh_sftp::client::error::Error) -> bool {
    matches!(e, russh_sftp::client::error::Error::Status(s) if s.status_code == StatusCode::Eof)
}
fn entry(path: VPath, a: FileAttributes) -> Result<Entry> {
    let kind = if a.file_type().is_dir() {
        FileKind::Directory
    } else if a.file_type().is_file() {
        FileKind::File
    } else {
        return Err(Error::Forbidden);
    };
    let size = a.size.unwrap_or(0);
    let modified = a.mtime.unwrap_or(0) as u64;
    Ok(Entry {
        name: path.name().into(),
        mime: mime_guess::from_path(path.name())
            .first_or_octet_stream()
            .to_string(),
        path,
        kind,
        size,
        modified,
        version: format!("{size}-{modified}-{}", a.permissions.unwrap_or(0)),
    })
}
pub use russh_sftp::client::error::Error as RawError;
pub use russh_sftp::protocol::{FileAttributes as Attrs, OpenFlags as Flags};
