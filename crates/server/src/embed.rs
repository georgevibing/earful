//! Embedding providers and the in-memory vector index used for semantic search.

use std::{collections::HashMap, sync::RwLock, time::Duration};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::json;

use crate::config::EmbedConfig;

pub enum Embedder {
    Disabled,
    Hash {
        dim: usize,
    },
    OpenAi {
        client: reqwest::Client,
        url: String,
        model: String,
        api_key: Option<String>,
    },
}

impl Embedder {
    pub fn from_config(cfg: &EmbedConfig) -> Result<Self> {
        Ok(match cfg {
            EmbedConfig::Disabled => Self::Disabled,
            EmbedConfig::Hash { dim } => Self::Hash {
                dim: (*dim).max(32),
            },
            EmbedConfig::OpenAi {
                url,
                model,
                api_key,
            } => Self::OpenAi {
                client: reqwest::Client::builder()
                    .timeout(Duration::from_secs(30))
                    .build()?,
                url: url.trim_end_matches('/').to_string(),
                model: model.clone(),
                api_key: api_key.clone(),
            },
        })
    }

    pub fn enabled(&self) -> bool {
        !matches!(self, Self::Disabled)
    }

    /// Identifies the vector space. Rows embedded under another model are re-embedded.
    pub fn model_id(&self) -> String {
        match self {
            Self::Disabled => "disabled".into(),
            Self::Hash { dim } => format!("hash-v1-{dim}"),
            Self::OpenAi { model, .. } => format!("openai:{model}"),
        }
    }

    /// Returns one L2-normalised vector per input.
    pub async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        match self {
            Self::Disabled => bail!("embeddings are disabled"),
            Self::Hash { dim } => Ok(texts.iter().map(|t| hash_embed(t, *dim)).collect()),
            Self::OpenAi {
                client,
                url,
                model,
                api_key,
            } => {
                #[derive(Deserialize)]
                struct Item {
                    index: usize,
                    embedding: Vec<f32>,
                }
                #[derive(Deserialize)]
                struct Resp {
                    data: Vec<Item>,
                }
                let mut req = client
                    .post(format!("{url}/embeddings"))
                    .json(&json!({ "model": model, "input": texts }));
                if let Some(key) = api_key {
                    req = req.bearer_auth(key);
                }
                let resp = req.send().await.context("embedding request failed")?;
                let status = resp.status();
                if !status.is_success() {
                    let body = resp.text().await.unwrap_or_default();
                    bail!(
                        "embedding provider returned {status}: {}",
                        truncate(&body, 300)
                    );
                }
                let mut parsed: Resp = resp.json().await.context("decoding embedding response")?;
                if parsed.data.len() != texts.len() {
                    bail!(
                        "embedding provider returned {} vectors for {} inputs",
                        parsed.data.len(),
                        texts.len()
                    );
                }
                parsed.data.sort_by_key(|i| i.index);
                Ok(parsed
                    .data
                    .into_iter()
                    .map(|i| {
                        let mut v = i.embedding;
                        normalize(&mut v);
                        v
                    })
                    .collect())
            }
        }
    }
}

fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Small concept map so the offline embedder groups common bug and feedback vocabulary.
const CONCEPTS: &[(&str, &[&str])] = &[
    (
        "crash",
        &[
            "crash",
            "freez",
            "froze",
            "hang",
            "stuck",
            "ctd",
            "segfault",
            "unresponsive",
            "lockup",
        ],
    ),
    (
        "perf",
        &[
            "lag", "stutter", "fps", "slow", "perform", "framerat", "laggy", "jank", "choppy",
        ],
    ),
    (
        "audio",
        &["sound", "audio", "music", "volum", "sfx", "mute"],
    ),
    (
        "visual",
        &[
            "graphic", "textur", "render", "glitch", "flicker", "shader", "visual",
        ],
    ),
    (
        "input",
        &[
            "control",
            "keyboard",
            "mous",
            "gamepad",
            "controller",
            "keybind",
            "input",
        ],
    ),
    ("save", &["save", "progress", "corrupt", "lost", "load"]),
    (
        "network",
        &[
            "server",
            "connect",
            "disconnect",
            "multiplayer",
            "ping",
            "netcod",
            "latenc",
            "timeout",
        ],
    ),
    (
        "ui",
        &["menu", "button", "ui", "interfac", "font", "layout", "hud"],
    ),
    (
        "praise",
        &[
            "love",
            "great",
            "awesom",
            "amazing",
            "fun",
            "enjoy",
            "fantastic",
            "brilliant",
        ],
    ),
    (
        "request",
        &[
            "pleas", "add", "wish", "would", "feature", "suggest", "could", "option",
        ],
    ),
    (
        "difficulty",
        &[
            "hard",
            "difficult",
            "easy",
            "balanc",
            "unfair",
            "overpow",
            "nerf",
            "buff",
        ],
    ),
];

fn stem(word: &str) -> &str {
    for suffix in ["ing", "edly", "ed", "es", "ly", "s", "e"] {
        if word.len() > suffix.len() + 2
            && let Some(stripped) = word.strip_suffix(suffix)
        {
            return stripped;
        }
    }
    word
}

fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn add_feature(v: &mut [f32], feature: &str, weight: f32) {
    let h = fnv1a(feature.as_bytes());
    let idx = (h % v.len() as u64) as usize;
    let sign = if h >> 63 == 0 { 1.0 } else { -1.0 };
    v[idx] += sign * weight;
}

/// Signed feature hashing over stemmed words, bigrams, character trigrams and concepts.
pub fn hash_embed(text: &str, dim: usize) -> Vec<f32> {
    let mut v = vec![0f32; dim];
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let stems: Vec<&str> = words.iter().map(|w| stem(w)).collect();
    for (i, s) in stems.iter().enumerate() {
        add_feature(&mut v, &format!("w:{s}"), 1.0);
        if let Some(next) = stems.get(i + 1) {
            add_feature(&mut v, &format!("b:{s} {next}"), 0.5);
        }
        let padded: Vec<char> = format!("^{s}$").chars().collect();
        for tri in padded.windows(3) {
            add_feature(
                &mut v,
                &format!("t:{}", tri.iter().collect::<String>()),
                0.25,
            );
        }
        for (concept, roots) in CONCEPTS {
            if roots.iter().any(|r| s.starts_with(r)) {
                add_feature(&mut v, &format!("c:{concept}"), 1.5);
            }
        }
    }
    normalize(&mut v);
    v
}

pub fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn from_blob(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

/// Characters sent to the embedder per item. Keeps every input well inside
/// provider context limits (8k tokens for OpenAI even for CJK text), because one
/// rejected input fails its whole batch and would stall the queue behind it.
pub const MAX_EMBED_CHARS: usize = 2000;

pub fn feedback_text(kind: &str, title: &str, body: &str) -> String {
    let text = format!("{kind}: {title}\n{body}");
    match text.char_indices().nth(MAX_EMBED_CHARS) {
        Some((idx, _)) => text[..idx].to_string(),
        None => text,
    }
}

/// Brute force cosine index. Thousands of vectors fit in a few MB and scan in microseconds.
#[derive(Default)]
pub struct VectorIndex {
    entries: RwLock<HashMap<i64, (i64, Vec<f32>)>>,
}

impl VectorIndex {
    pub fn insert(&self, id: i64, project_id: i64, vec: Vec<f32>) {
        self.entries.write().unwrap().insert(id, (project_id, vec));
    }

    pub fn remove(&self, id: i64) {
        self.entries.write().unwrap().remove(&id);
    }

    pub fn remove_project(&self, project_id: i64) {
        self.entries
            .write()
            .unwrap()
            .retain(|_, (p, _)| *p != project_id);
    }

    pub fn clear(&self) {
        self.entries.write().unwrap().clear();
    }

    pub fn len(&self) -> usize {
        self.entries.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, id: i64) -> Option<Vec<f32>> {
        self.entries
            .read()
            .unwrap()
            .get(&id)
            .map(|(_, v)| v.clone())
    }

    /// Top `k` ids by cosine similarity, optionally restricted to a project.
    pub fn search(
        &self,
        query: &[f32],
        project: Option<i64>,
        k: usize,
        exclude: Option<i64>,
    ) -> Vec<(i64, f32)> {
        let entries = self.entries.read().unwrap();
        let mut scored: Vec<(i64, f32)> = entries
            .iter()
            .filter(|(id, (p, v))| {
                project.is_none_or(|want| *p == want)
                    && Some(**id) != exclude
                    && v.len() == query.len()
            })
            .map(|(id, (_, v))| (*id, dot(query, v)))
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.truncate(k);
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_embedding_is_normalised_and_deterministic() {
        let a = hash_embed("The game crashes on startup", 256);
        let b = hash_embed("The game crashes on startup", 256);
        assert_eq!(a, b);
        assert!((dot(&a, &a) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn related_texts_score_higher_than_unrelated() {
        let q = hash_embed("game freezes", 384);
        let related = hash_embed("it crashed and got stuck on the loading screen", 384);
        let unrelated = hash_embed("I love the soundtrack and the art style", 384);
        assert!(dot(&q, &related) > dot(&q, &unrelated));
    }

    #[test]
    fn feedback_text_is_bounded() {
        let long = "漢".repeat(10_000);
        let t = feedback_text("bug", "title", &long);
        assert_eq!(t.chars().count(), MAX_EMBED_CHARS);
        assert_eq!(feedback_text("bug", "t", "b"), "bug: t\nb");
    }

    #[test]
    fn blob_round_trip() {
        let v = vec![0.5, -1.25, 3.0];
        assert_eq!(from_blob(&to_blob(&v)), v);
    }

    #[test]
    fn index_filters_by_project_and_excludes_self() {
        let idx = VectorIndex::default();
        idx.insert(1, 10, vec![1.0, 0.0]);
        idx.insert(2, 10, vec![0.8, 0.6]);
        idx.insert(3, 20, vec![1.0, 0.0]);
        let hits = idx.search(&[1.0, 0.0], Some(10), 5, Some(1));
        assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), vec![2]);
        idx.remove_project(10);
        assert_eq!(idx.len(), 1);
    }
}
