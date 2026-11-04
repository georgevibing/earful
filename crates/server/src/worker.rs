//! Background jobs: embedding backfill, near duplicate detection and pruning.

use std::time::Duration;

use anyhow::Result;
use rusqlite::params;

use crate::{
    db::now,
    embed::{self, dot},
    ratelimit,
    state::AppState,
};

const BATCH: usize = 32;
/// Cosine similarity above which two submissions from the same IP count as the same text.
pub const NEAR_DUP_SIMILARITY: f32 = 0.95;

pub fn spawn(state: AppState) {
    if state.embedder.enabled() {
        let s = state.clone();
        tokio::spawn(async move { embed_loop(s).await });
    }
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tick.tick().await;
            if let Err(e) = prune(&state).await {
                tracing::warn!(error = ?e, "prune failed");
            }
        }
    });
}

async fn embed_loop(state: AppState) {
    let mut backoff = Duration::from_secs(5);
    loop {
        match embed_pending(&state).await {
            Ok(0) => {
                backoff = Duration::from_secs(5);
                tokio::select! {
                    _ = state.embed_notify.notified() => {}
                    _ = tokio::time::sleep(Duration::from_secs(300)) => {}
                }
            }
            Ok(_) => continue,
            Err(e) => {
                tracing::warn!(error = %e, retry_in = ?backoff, "embedding batch failed");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(600));
            }
        }
    }
}

/// Embeds one batch of rows lacking a vector for the current model. Returns how many were embedded.
pub async fn embed_pending(state: &AppState) -> Result<usize> {
    let model = state.embedder.model_id();
    let m = model.clone();
    let pending: Vec<(i64, i64, String)> = state
        .db
        .call(move |c| {
            let mut stmt = c.prepare(
                "SELECT f.id, f.project_id, f.kind, f.title, f.body FROM feedback f
                 LEFT JOIN embeddings e ON e.feedback_id = f.id AND e.model = ?1
                 WHERE e.feedback_id IS NULL ORDER BY f.id LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![m, BATCH as i64], |r| {
                let kind: String = r.get(2)?;
                let title: String = r.get(3)?;
                let body: String = r.get(4)?;
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    embed::feedback_text(&kind, &title, &body),
                ))
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await?;
    if pending.is_empty() {
        return Ok(0);
    }

    let texts: Vec<String> = pending.iter().map(|p| p.2.clone()).collect();
    let vectors = state.embedder.embed(&texts).await?;
    let rows: Vec<(i64, i64, Vec<f32>)> = pending
        .into_iter()
        .zip(vectors)
        .map(|((id, project_id, _), v)| (id, project_id, v))
        .collect();

    let to_store = rows.clone();
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            for (id, _, v) in &to_store {
                tx.execute(
                    "INSERT INTO embeddings (feedback_id, model, vec) VALUES (?1, ?2, ?3)
                     ON CONFLICT (feedback_id) DO UPDATE SET model = excluded.model, vec = excluded.vec",
                    params![id, model, embed::to_blob(v)],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
        .await?;

    let count = rows.len();
    for (id, project_id, v) in rows {
        state.index.insert(id, project_id, v);
        flag_near_duplicate(state, id).await?;
    }
    Ok(count)
}

/// Quarantines a fresh submission that is nearly identical to another one sent
/// from the same IP within the last day (paraphrased or lightly edited spam).
async fn flag_near_duplicate(state: &AppState, id: i64) -> Result<()> {
    let Some(vec) = state.index.get(id) else {
        return Ok(());
    };
    let siblings: Vec<i64> = state
        .db
        .call(move |c| {
            let mut stmt = c.prepare(
                "SELECT o.id FROM feedback f JOIN feedback o
                   ON o.ip_hash = f.ip_hash AND o.project_id = f.project_id AND o.id != f.id
                 WHERE f.id = ?1 AND f.status = 'new' AND f.dup_checked = 0
                   AND f.kind IN ('bug', 'feedback') AND o.kind = f.kind
                   AND o.created_at > f.created_at - 86400",
            )?;
            let rows = stmt.query_map(params![id], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<Vec<i64>>>()?)
        })
        .await?;
    let is_dup = siblings
        .iter()
        .filter_map(|sid| state.index.get(*sid))
        .any(|other| dot(&vec, &other) >= NEAR_DUP_SIMILARITY);
    let ts = now();
    state
        .db
        .call(move |c| {
            c.execute(
                "UPDATE feedback SET dup_checked = 1 WHERE id = ?1",
                params![id],
            )?;
            if !is_dup {
                return Ok(());
            }
            let changed = c.execute(
                "UPDATE feedback SET status = 'spam', spam_reason = 'near_duplicate', updated_at = ?2
                 WHERE id = ?1 AND status = 'new'",
                params![id, ts],
            )?;
            if changed > 0 {
                let project_id: i64 = c.query_row(
                    "SELECT project_id FROM feedback WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )?;
                ratelimit::record_block(c, Some(project_id), "near_duplicate", ts)?;
            }
            Ok(())
        })
        .await?;
    Ok(())
}

pub async fn prune(state: &AppState) -> Result<()> {
    let ts = now();
    state
        .db
        .call(move |c| {
            c.execute(
                "DELETE FROM rate_events WHERE ts < ?1",
                params![ts - 2 * 86400],
            )?;
            c.execute(
                "DELETE FROM used_challenges WHERE expires_at < ?1",
                params![ts],
            )?;
            c.execute("DELETE FROM sessions WHERE expires_at < ?1", params![ts])?;
            c.execute(
                "DELETE FROM block_counts WHERE day < ?1",
                params![ts / 86400 - 90],
            )?;
            Ok(())
        })
        .await
}
