mod config;
mod error;
mod files;
mod maintenance;
mod range;
mod security;
mod store;
mod uploads;
use crate::{config::Config, store::Store};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, patch, post},
};
use std::{
    collections::HashMap,
    future::IntoFuture,
    sync::{Arc, Mutex, Weak},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex as AsyncMutex, Semaphore, broadcast};
use tokio_util::sync::CancellationToken;
use tower_http::services::ServeDir;
#[derive(Clone)]
pub struct App(Arc<Inner>);
pub struct Inner {
    config: Config,
    pool: glassftp_sftp::Pool,
    store: Store,
    mutation: AsyncMutex<()>,
    transfers: Arc<Semaphore>,
    locks: Mutex<HashMap<String, Weak<AsyncMutex<()>>>>,
    events: broadcast::Sender<String>,
    shutdown: CancellationToken,
}
impl std::ops::Deref for App {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}
impl App {
    fn changed(&self, path: &str) {
        let _ = self
            .events
            .send(serde_json::json!({"path": path}).to_string());
        tracing::info!(operation = "changed", path = path);
    }
    fn transfer_lock(&self, id: &str) -> error::Result<Arc<AsyncMutex<()>>> {
        let mut locks = self.locks.lock().map_err(|_| error::Error::internal())?;
        locks.retain(|_, v| v.strong_count() > 0);
        if let Some(lock) = locks.get(id).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        let lock = Arc::new(AsyncMutex::new(()));
        locks.insert(id.into(), Arc::downgrade(&lock));
        Ok(lock)
    }
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn router(app: App) -> Router {
    Router::new()
        .route("/health", get(files::health))
        .route("/api/v1/files", get(files::list))
        .route("/api/v1/files/info", get(files::info))
        .route("/api/v1/folders", post(files::mkdir))
        .route("/api/v1/files/move", post(files::move_file))
        .route("/api/v1/files/rename", patch(files::move_file))
        .route("/api/v1/files/copy", post(files::copy))
        .route("/api/v1/files/trash", post(files::trash))
        .route("/api/v1/trash", get(files::trash_list))
        .route("/api/v1/trash/{id}/restore", post(files::restore))
        .route("/api/v1/trash/{id}", axum::routing::delete(files::purge))
        .route("/api/v1/text", get(files::text).put(files::save_text))
        .route("/api/v1/download", get(files::download))
        .route("/api/v1/events", get(files::events))
        .route("/api/v1/uploads", post(uploads::create).get(uploads::list))
        .route(
            "/api/v1/uploads/{id}",
            get(uploads::status)
                .patch(uploads::chunk)
                .delete(uploads::cancel),
        )
        .route("/api/v1/uploads/{id}/complete", post(uploads::complete))
        .fallback_service(ServeDir::new(&app.config.assets))
        .layer(DefaultBodyLimit::max(uploads::CHUNK_LIMIT))
        .layer(middleware::from_fn_with_state(app.clone(), security::guard))
        .with_state(app)
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "glassftp_server=info".into()),
        )
        .json()
        .init();
    let config = Config::load().map_err(std::io::Error::other)?;
    let pool = glassftp_sftp::Pool::new(config.sftp.clone());
    let store = Store::open(&config.database).map_err(|_| "Cannot open metadata database")?;
    let (events, _) = broadcast::channel(128);
    let shutdown = CancellationToken::new();
    let addr = config.listen;
    let app = App(Arc::new(Inner {
        transfers: Arc::new(Semaphore::new(config.transfers)),
        config,
        pool,
        store,
        events,
        shutdown: shutdown.clone(),
        mutation: AsyncMutex::new(()),
        locks: Mutex::new(HashMap::new()),
    }));
    // Fail closed if key, password, root, fsync or internal directory setup is wrong.
    let c = app.pool.get().await?;
    c.prepare().await?;
    c.recycle();
    tokio::spawn(maintenance::run(app.clone()));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(listen = %addr, "GlassFTP ready");
    let requested = shutdown.clone();
    let server = axum::serve(listener, router(app))
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            requested.cancel();
            tracing::info!("Shutdown requested; waiting for active HTTP requests");
        })
        .into_future();
    tokio::select! {
        result = server => result?,
        _ = async {
            shutdown.cancelled().await;
            tokio::time::sleep(Duration::from_secs(75)).await;
        } => {
            tracing::warn!("Shutdown drain deadline exceeded; incomplete transfers retain their journal");
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "HTTP shutdown deadline exceeded").into());
        }
    }
    tracing::info!("HTTP server stopped");
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => tokio::select! {
                _ = tokio::signal::ctrl_c() => (),
                _ = terminate.recv() => (),
            },
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
