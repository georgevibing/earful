use std::sync::Arc;

use anyhow::Result;
use rusqlite::params;
use tokio::sync::Notify;

use crate::{
    config::Config,
    db::{self, Db},
    embed::{self, Embedder, VectorIndex},
};

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub db: Db,
    pub secret: Arc<Vec<u8>>,
    pub embedder: Arc<Embedder>,
    pub index: Arc<VectorIndex>,
    /// Wakes the embedding worker when new feedback arrives.
    pub embed_notify: Arc<Notify>,
}

impl AppState {
    pub async fn init(cfg: Config) -> Result<Self> {
        let mut conn = db::open(&cfg.database_path)?;
        db::migrate(&mut conn)?;
        let secret = db::server_secret(&conn)?;
        let embedder = Embedder::load(&cfg.embed).await?;
        let index = VectorIndex::default();
        load_index(&conn, &embedder, &index)?;
        Ok(Self {
            cfg: Arc::new(cfg),
            db: Db::new(conn),
            secret: Arc::new(secret),
            embedder: Arc::new(embedder),
            index: Arc::new(index),
            embed_notify: Arc::new(Notify::new()),
        })
    }

    pub fn ip_hash(&self, ip: &str) -> String {
        crate::crypto::ip_hash(&self.secret, ip)
    }
}

fn load_index(conn: &rusqlite::Connection, embedder: &Embedder, index: &VectorIndex) -> Result<()> {
    if !embedder.enabled() {
        return Ok(());
    }
    let mut stmt = conn.prepare(
        "SELECT e.feedback_id, f.project_id, e.vec FROM embeddings e
         JOIN feedback f ON f.id = e.feedback_id WHERE e.model = ?1 AND length(e.vec) > 0",
    )?;
    let rows = stmt.query_map(params![embedder.model_id()], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Vec<u8>>(2)?,
        ))
    })?;
    for row in rows {
        let (id, project_id, blob) = row?;
        index.insert(id, project_id, embed::from_blob(&blob));
    }
    tracing::info!(vectors = index.len(), model = %embedder.model_id(), "loaded vector index");
    Ok(())
}
