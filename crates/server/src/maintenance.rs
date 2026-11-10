//! Conservative retention: never touch a file whose rename outcome is uncertain.
use crate::{App, now};
use glassftp_shared::Upload;
use std::time::Duration;
pub async fn run(app: App) {
    let mut timer = tokio::time::interval(Duration::from_secs(15 * 60));
    loop {
        timer.tick().await;
        if let Err(error) = sweep(&app).await {
            tracing::warn!(code = error.1, "upload_cleanup_failed");
        }
    }
}
async fn sweep(app: &App) -> crate::error::Result<()> {
    for upload in app.store.list::<Upload>("uploads").await? {
        let age = now().saturating_sub(upload.updated.max(upload.created));
        if upload.status == "committing" || age < 24 * 3600 {
            continue;
        }
        if upload.status != "complete" && age < 7 * 24 * 3600 {
            continue;
        }
        let lock = app.transfer_lock(&upload.id)?;
        let Ok(_guard) = lock.try_lock() else {
            continue;
        };
        // Re-read under the per-transfer lock; a chunk may have refreshed the timestamp.
        let latest: Upload = app.store.get("uploads", &upload.id).await?;
        if latest.updated != upload.updated || latest.status != upload.status {
            continue;
        }
        if upload.status == "active" {
            let c = app.pool.get().await?;
            match c
                .raw
                .remove(c.internal("uploads", crate::uploads::id(&upload.id)?))
                .await
            {
                Ok(_) => (),
                Err(e) if glassftp_sftp::missing(&e) => (),
                Err(e) => return Err(e.into()),
            }
            c.recycle();
        } else if upload.status != "complete" {
            continue;
        }
        app.store.delete("uploads", &upload.id).await?;
    }
    Ok(())
}
