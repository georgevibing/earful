//! Model2Vec provider tests. The real-model tests only run when
//! `EARFUL_TEST_MODEL2VEC_PATH` points at a downloaded model, e.g.
//! `earful download-model && EARFUL_TEST_MODEL2VEC_PATH=models/potion-base-8M cargo test --test model2vec -- --nocapture`

mod common;

use std::{path::PathBuf, time::Instant};

use common::{App, key_of, titles};
use earful::{
    config::{Config, EmbedConfig},
    embed::{self, Embedder},
    state::AppState,
    worker,
};

/// Realistic feedback where the right match often shares no words with the query.
const CORPUS: &[(&str, &str, &str)] = &[
    (
        "bug",
        "Black screen on launch",
        "It launches to a black screen and closes after a second.",
    ),
    (
        "bug",
        "Sound out of sync",
        "The sound is behind the video in every cutscene.",
    ),
    (
        "bug",
        "Progress gone",
        "My save file disappeared after the update, 20 hours lost.",
    ),
    (
        "feedback",
        "Too punishing",
        "The second boss is way too hard, I died 40 times.",
    ),
    (
        "bug",
        "Choppy on laptop",
        "Frame rate drops to 15 on my laptop in the forest area.",
    ),
    (
        "feedback",
        "Love the art",
        "The hand drawn art style is gorgeous, great work.",
    ),
    (
        "bug",
        "Cannot rebind keys",
        "There is no way to change keyboard bindings in the settings.",
    ),
    (
        "bug",
        "Kicked from lobby",
        "I keep getting disconnected from multiplayer matches after a few minutes.",
    ),
    (
        "bug",
        "Text cut off",
        "German translation overflows the buttons in the main menu.",
    ),
    (
        "bug",
        "Fell out of the world",
        "Walked into a wall near the river and dropped below the map.",
    ),
    (
        "feedback",
        "Price feels high",
        "30 dollars is a lot for four hours of content.",
    ),
    (
        "bug",
        "Trophy not awarded",
        "Beat the final boss but the Steam trophy never popped.",
    ),
    (
        "bug",
        "Phone gets hot",
        "Ten minutes of play heats up my phone and empties the charge.",
    ),
    (
        "bug",
        "Sign in fails",
        "The account page says invalid credentials even with the right password.",
    ),
    (
        "feedback",
        "Accessibility",
        "I cannot tell the red and green markers apart, I have deuteranopia.",
    ),
    (
        "bug",
        "Charged twice",
        "My card was billed two times for the same DLC purchase.",
    ),
    (
        "feedback",
        "Story ending",
        "The ending felt rushed, the last chapter needs more cutscenes.",
    ),
    (
        "bug",
        "Chat broken",
        "Messages in party chat never arrive for my friends.",
    ),
];

const QUERIES: &[(&str, &str, bool)] = &[
    // (query, expected title, uses words from the hash embedder's concept list)
    ("game won't start", "Black screen on launch", false),
    ("lip sync is off", "Sound out of sync", false),
    ("lost my saved game", "Progress gone", true),
    ("difficulty is unfair", "Too punishing", true),
    ("bad performance low fps", "Choppy on laptop", true),
    ("remap controls", "Cannot rebind keys", true),
    ("connection drops online", "Kicked from lobby", true),
    ("localization problems", "Text cut off", false),
    (
        "character clipped through the floor",
        "Fell out of the world",
        false,
    ),
    ("too expensive for what you get", "Price feels high", false),
    ("achievement didn't unlock", "Trophy not awarded", false),
    ("battery drain on mobile", "Phone gets hot", false),
    ("can't log into my account", "Sign in fails", false),
    ("colorblind mode please", "Accessibility", false),
    ("refund for double payment", "Charged twice", false),
    (
        "narrative conclusion was disappointing",
        "Story ending",
        false,
    ),
    ("my friends don't receive my messages", "Chat broken", false),
];

fn model_path() -> Option<PathBuf> {
    std::env::var("EARFUL_TEST_MODEL2VEC_PATH")
        .ok()
        .map(PathBuf::from)
}

async fn model2vec() -> Option<Embedder> {
    let Some(path) = model_path() else {
        eprintln!("skipped: set EARFUL_TEST_MODEL2VEC_PATH to run");
        return None;
    };
    let started = Instant::now();
    let e = Embedder::load(&EmbedConfig::Model2Vec {
        path,
        repo: String::new(),
        auto_download: false,
    })
    .await
    .unwrap();
    println!("model2vec load: {:?}", started.elapsed());
    Some(e)
}

struct Eval {
    mrr: f64,
    top1_uncovered: usize,
    matched_median: f32,
    other_median: f32,
}

/// Ranks the whole corpus by raw cosine for every query.
async fn evaluate(name: &str, e: &Embedder) -> Eval {
    let docs: Vec<String> = CORPUS
        .iter()
        .map(|(k, t, b)| embed::feedback_text(k, t, b))
        .collect();
    let doc_vecs = e.embed(&docs).await.unwrap();
    let (mut mrr, mut top1_uncovered) = (0.0, 0);
    let (mut matched, mut other) = (Vec::new(), Vec::new());
    for (q, expected, covered) in QUERIES {
        let qv = e.embed(&[q.to_string()]).await.unwrap().remove(0);
        let mut scored: Vec<(f32, &str)> = CORPUS
            .iter()
            .zip(&doc_vecs)
            .map(|((_, t, _), d)| (embed::dot(&qv, d), *t))
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let rank = scored.iter().position(|s| s.1 == *expected).unwrap() + 1;
        for (s, t) in &scored {
            if t == expected {
                matched.push(*s)
            } else {
                other.push(*s)
            }
        }
        mrr += 1.0 / rank as f64;
        top1_uncovered += usize::from(rank == 1 && !covered);
        println!("  {name:<10} rank {rank:>2}  {q}");
    }
    matched.sort_by(f32::total_cmp);
    other.sort_by(f32::total_cmp);
    let eval = Eval {
        mrr: mrr / QUERIES.len() as f64,
        top1_uncovered,
        matched_median: matched[matched.len() / 2],
        other_median: other[other.len() / 2],
    };
    println!(
        "{name}: MRR {:.3}, top-1 on uncovered vocabulary {}/{}, cosine matched median {:.3} other median {:.3} other p95 {:.3}",
        eval.mrr,
        eval.top1_uncovered,
        QUERIES.iter().filter(|q| !q.2).count(),
        eval.matched_median,
        eval.other_median,
        other[other.len() * 95 / 100],
    );
    eval
}

#[tokio::test]
async fn model2vec_beats_the_hash_embedder_on_paraphrases() {
    let Some(m2v) = model2vec().await else { return };
    let hash = Embedder::load(&EmbedConfig::Hash { dim: 384 })
        .await
        .unwrap();
    let h = evaluate("hash", &hash).await;
    let m = evaluate("model2vec", &m2v).await;
    assert!(
        m.mrr > h.mrr + 0.1,
        "model2vec MRR {:.3} vs hash {:.3}",
        m.mrr,
        h.mrr
    );
    assert!(m.top1_uncovered > h.top1_uncovered);
    let floor = m2v.semantic_floor();
    assert!(
        floor < m.matched_median && floor > m.other_median,
        "floor {floor} sits between the distributions"
    );
}

#[tokio::test]
async fn model2vec_semantic_search_end_to_end_and_latency() {
    let Some(path) = model_path() else {
        eprintln!("skipped: set EARFUL_TEST_MODEL2VEC_PATH to run");
        return;
    };
    let mut app = App::with(move |c| {
        c.embed = EmbedConfig::Model2Vec {
            path,
            repo: String::new(),
            auto_download: false,
        }
    })
    .await;
    app.login().await;
    let project = app.create_project("Quality").await;
    let key = key_of(&project);
    let pid = project["id"].as_i64().unwrap();
    for (i, (kind, title, body)) in CORPUS.iter().enumerate() {
        app.submit_ok(&key, &format!("10.0.0.{i}"), kind, title, body)
            .await;
    }
    app.embed_all().await;
    for (q, expected) in [
        ("achievement didn't unlock", "Trophy not awarded"),
        ("can't log into my account", "Sign in fails"),
        ("refund for double payment", "Charged twice"),
        ("my friends don't receive my messages", "Chat broken"),
    ] {
        for mode in ["semantic", "hybrid"] {
            let r = app
                .feedback(&format!(
                    "mode={mode}&status=any&project_id={pid}&q={}",
                    q.replace(' ', "+")
                ))
                .await;
            assert_eq!(r["semantic"], true);
            assert_eq!(
                titles(&r).first().map(String::as_str),
                Some(expected),
                "{mode}: {q}"
            );
        }
    }

    let m2v = model2vec().await.unwrap();
    let batch: Vec<String> = (0..1000)
        .map(|i| format!("{} variant {i}", CORPUS[i % CORPUS.len()].2))
        .collect();
    let started = Instant::now();
    m2v.embed(&batch).await.unwrap();
    println!(
        "model2vec embed: {:?} per item (batch of 1000)",
        started.elapsed() / 1000
    );
}

/// Changing the embedder (provider or model) changes the model id, and the
/// worker re-embeds every row into the new vector space.
#[tokio::test]
async fn switching_embedders_reembeds_everything() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("switch.db");
    let mut cfg = Config::for_tests(db.clone());
    cfg.embed = EmbedConfig::Hash { dim: 384 };
    let first = AppState::init(cfg).await.unwrap();
    first
        .db
        .call(|c| {
            c.execute_batch(
                "INSERT INTO projects (slug, title, ingest_key, created_at, updated_at)
                 VALUES ('p', 'P', 'ear_x', 0, 0);
                 INSERT INTO feedback (project_id, kind, title, body, content_hash, ip_hash, created_at, updated_at, last_seen_at)
                 VALUES (1, 'bug', 'a', 'first body', 'h1', 'ip', 0, 0, 0),
                        (1, 'bug', 'b', 'second body', 'h2', 'ip', 0, 0, 0);",
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(worker::embed_pending(&first).await.unwrap(), 2);
    assert_eq!(worker::embed_pending(&first).await.unwrap(), 0);
    drop(first);

    let mut cfg = Config::for_tests(db);
    cfg.embed = EmbedConfig::Hash { dim: 128 };
    let second = AppState::init(cfg).await.unwrap();
    assert_eq!(
        second.index.len(),
        0,
        "old vectors are not loaded into the new space"
    );
    assert_eq!(worker::embed_pending(&second).await.unwrap(), 2);
    let models: Vec<String> = second
        .db
        .call(|c| {
            let mut s = c.prepare("SELECT DISTINCT model FROM embeddings")?;
            let rows = s.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<Vec<String>>>()?)
        })
        .await
        .unwrap();
    assert_eq!(models, vec!["hash-v1-128".to_string()]);
    assert_eq!(second.index.len(), 2);
}

#[tokio::test]
async fn missing_model_fails_with_a_helpful_error() {
    let err = Embedder::load(&EmbedConfig::Model2Vec {
        path: "/definitely/missing/potion".into(),
        repo: "minishlab/potion-base-8M".into(),
        auto_download: false,
    })
    .await
    .err()
    .expect("missing model must fail");
    let msg = format!("{err:#}");
    assert!(msg.contains("download-model"), "{msg}");
    assert!(msg.contains("EARFUL_EMBED_AUTO_DOWNLOAD"), "{msg}");
}
