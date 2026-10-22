//! Keyword (BM25), fuzzy (trigram + edit distance) and rank fusion.
//! Semantic candidates come from [`crate::embed::VectorIndex`].

use std::collections::HashMap;

use rusqlite::{Connection, params};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Hybrid,
    Fuzzy,
    Keyword,
    Semantic,
}

const CANDIDATES: usize = 300;

fn words(q: &str) -> Vec<String> {
    q.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .take(16)
        .collect()
}

/// BM25 over the stemmed word index. Each term is a prefix match, OR'ed together.
pub fn keyword(conn: &Connection, q: &str) -> rusqlite::Result<Vec<(i64, f64)>> {
    let terms = words(q);
    if terms.is_empty() {
        return Ok(vec![]);
    }
    let fts = terms
        .iter()
        .map(|t| format!("\"{t}\"*"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let mut stmt = conn.prepare(
        "SELECT rowid, bm25(feedback_fts, 3.0, 1.0, 0.5) AS r FROM feedback_fts
         WHERE feedback_fts MATCH ?1 ORDER BY r LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![fts, CANDIDATES as i64], |r| {
        Ok((r.get(0)?, -r.get::<_, f64>(1)?))
    })?;
    rows.collect()
}

/// Typo tolerant search. Trigram index finds candidates sharing any trigram with
/// the query, then each candidate is scored by best per-word similarity.
pub fn fuzzy(conn: &Connection, q: &str) -> rusqlite::Result<Vec<(i64, f64)>> {
    let terms = words(q);
    if terms.is_empty() {
        return Ok(vec![]);
    }
    let mut trigrams: Vec<String> = terms
        .iter()
        .flat_map(|t| {
            let chars: Vec<char> = t.chars().collect();
            chars
                .windows(3)
                .map(|w| w.iter().collect::<String>())
                .collect::<Vec<_>>()
        })
        .collect();
    trigrams.sort();
    trigrams.dedup();

    let candidates: Vec<(i64, String, String)> = if trigrams.is_empty() {
        // Every term is shorter than three chars: fall back to a substring scan.
        let pattern = format!("%{}%", terms.join(" ").replace(['%', '_'], ""));
        let mut stmt = conn.prepare(
            "SELECT id, title, body FROM feedback WHERE title LIKE ?1 OR body LIKE ?1
             ORDER BY created_at DESC LIMIT ?2",
        )?;
        stmt.query_map(params![pattern, CANDIDATES as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<rusqlite::Result<_>>()?
    } else {
        let fts = trigrams
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(" OR ");
        let mut stmt = conn.prepare(
            "SELECT f.id, f.title, f.body FROM feedback_tri t JOIN feedback f ON f.id = t.rowid
             WHERE feedback_tri MATCH ?1 ORDER BY t.rank LIMIT ?2",
        )?;
        stmt.query_map(params![fts, CANDIDATES as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<rusqlite::Result<_>>()?
    };

    let phrase = terms.join(" ");
    let mut scored: Vec<(i64, f64)> = candidates
        .into_iter()
        .filter_map(|(id, title, body)| {
            let title_l = title.to_lowercase();
            let body_l = body.to_lowercase();
            let doc_words: std::collections::HashSet<String> = words_unbounded(&title_l)
                .chain(words_unbounded(&body_l))
                .collect();
            let mut total = 0.0;
            for term in &terms {
                let best = doc_words
                    .iter()
                    .map(|w| word_similarity(term, w))
                    .fold(0.0, f64::max);
                total += best;
            }
            let mut score = total / terms.len() as f64;
            if title_l.contains(&phrase) {
                score += 0.3;
            } else if body_l.contains(&phrase) {
                score += 0.15;
            }
            (score >= 0.6).then_some((id, score))
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    Ok(scored)
}

fn words_unbounded(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
}

/// 1.0 for equal words, high for prefixes, otherwise normalised edit distance.
pub fn word_similarity(query: &str, word: &str) -> f64 {
    if query == word {
        return 1.0;
    }
    let ql = query.chars().count();
    if ql >= 3 && word.starts_with(query) {
        return 0.95;
    }
    let wl = word.chars().count();
    if ql.abs_diff(wl) > 3 {
        return 0.0;
    }
    let dist = damerau_levenshtein(query, word);
    let max_typos = match ql {
        0..=3 => 0,
        4..=5 => 1,
        6..=8 => 2,
        _ => 3,
    };
    if dist > max_typos {
        return 0.0;
    }
    1.0 - dist as f64 / ql.max(wl) as f64 * 0.8
}

/// Optimal string alignment distance (adjacent transpositions count as one edit).
fn damerau_levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[n][m]
}

/// Reciprocal rank fusion. Robust to the very different score scales of BM25,
/// edit distance similarity and cosine similarity.
pub fn rrf(lists: &[Vec<(i64, f64)>]) -> Vec<(i64, f64)> {
    const K: f64 = 60.0;
    let mut acc: HashMap<i64, f64> = HashMap::new();
    for list in lists {
        for (rank, (id, _)) in list.iter().enumerate() {
            *acc.entry(*id).or_default() += 1.0 / (K + rank as f64 + 1.0);
        }
    }
    let mut out: Vec<(i64, f64)> = acc.into_iter().collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(b.0.cmp(&a.0)));
    out
}

/// Keeps semantic hits above an absolute floor and close to the best hit.
pub fn cut_semantic(hits: Vec<(i64, f32)>, floor: f32) -> Vec<(i64, f64)> {
    let top = hits.first().map(|h| h.1).unwrap_or(0.0);
    hits.into_iter()
        .filter(|(_, s)| *s >= floor && *s >= top * 0.7)
        .take(100)
        .map(|(id, s)| (id, s as f64))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarity_handles_typos_and_prefixes() {
        assert_eq!(word_similarity("crash", "crash"), 1.0);
        assert!(word_similarity("inven", "inventory") > 0.9);
        assert!(word_similarity("crahs", "crash") > 0.7);
        assert!(word_similarity("invetnory", "inventory") > 0.7);
        assert_eq!(word_similarity("cat", "car"), 0.0);
        assert_eq!(word_similarity("crash", "banana"), 0.0);
    }

    #[test]
    fn osa_distance() {
        assert_eq!(damerau_levenshtein("abc", "abc"), 0);
        assert_eq!(damerau_levenshtein("abc", "acb"), 1);
        assert_eq!(damerau_levenshtein("kitten", "sitting"), 3);
    }

    #[test]
    fn rrf_rewards_agreement() {
        let a = vec![(1, 9.0), (2, 5.0)];
        let b = vec![(2, 0.9), (3, 0.8)];
        let fused = rrf(&[a, b]);
        assert_eq!(fused[0].0, 2);
    }

    #[test]
    fn semantic_cut_is_relative_and_absolute() {
        let hits = vec![(1, 0.9), (2, 0.7), (3, 0.5), (4, 0.05)];
        let kept: Vec<i64> = cut_semantic(hits, 0.1).into_iter().map(|h| h.0).collect();
        assert_eq!(kept, vec![1, 2]);
    }
}
