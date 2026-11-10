//! Small local journal. All SQLite work is off the async executor.
use crate::error::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
#[derive(Clone)]
pub struct Store(Arc<Mutex<Connection>>);
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let c = Connection::open(path).map_err(|_| Error::internal())?;
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS records(kind TEXT NOT NULL, id TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(kind,id));").map_err(|_| Error::internal())?;
        Ok(Self(Arc::new(Mutex::new(c))))
    }
    async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
    ) -> Result<T> {
        let c = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let guard = c.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
            f(&guard)
        })
        .await
        .map_err(|_| Error::internal())?
        .map_err(|e| {
            tracing::error!(error = %e, "metadata_store");
            Error::internal()
        })
    }
    pub async fn put<T: Serialize>(&self, kind: &'static str, id: &str, value: &T) -> Result<()> {
        let id = id.to_owned();
        let json = serde_json::to_string(value).map_err(|_| Error::internal())?;
        self.run(move |c| { c.execute("INSERT INTO records VALUES (?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET value=excluded.value", params![kind,id,json])?; Ok(()) }).await
    }
    pub async fn get<T: DeserializeOwned>(&self, kind: &'static str, id: &str) -> Result<T> {
        let id = id.to_owned();
        let json: Option<String> = self
            .run(move |c| {
                c.query_row(
                    "SELECT value FROM records WHERE kind=?1 AND id=?2",
                    params![kind, id],
                    |r| r.get(0),
                )
                .optional()
            })
            .await?;
        serde_json::from_str(&json.ok_or_else(Error::not_found)?).map_err(|_| Error::internal())
    }
    pub async fn list<T: DeserializeOwned>(&self, kind: &'static str) -> Result<Vec<T>> {
        let items: Vec<String> = self
            .run(move |c| {
                c.prepare("SELECT value FROM records WHERE kind=?1 ORDER BY id LIMIT 10001")?
                    .query_map([kind], |r| r.get(0))?
                    .collect()
            })
            .await?;
        if items.len() > 10000 {
            return Err(Error::bad(
                "Plus de 10 000 métadonnées : archivage administrateur requis",
            ));
        }
        items
            .iter()
            .map(|s| serde_json::from_str(s).map_err(|_| Error::internal()))
            .collect()
    }
    pub async fn delete(&self, kind: &'static str, id: &str) -> Result<()> {
        let id = id.to_owned();
        self.run(move |c| {
            c.execute(
                "DELETE FROM records WHERE kind=?1 AND id=?2",
                params![kind, id],
            )?;
            Ok(())
        })
        .await
    }
}
