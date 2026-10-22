//! Content heuristics, normalisation and client IP resolution.

use std::net::{IpAddr, SocketAddr};

use axum::http::HeaderMap;

use crate::crypto;

/// Strips control characters (keeping newlines and tabs) and trims.
pub fn sanitize(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .trim()
        .to_string()
}

/// Case and whitespace insensitive fingerprint used for exact duplicate detection.
pub fn content_hash(title: &str, body: &str) -> String {
    let normalized: String = format!("{title}\n{body}")
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    crypto::sha256_hex(normalized.as_bytes())
}

const SPAM_PHRASES: &[&str] = &[
    "casino",
    "viagra",
    "cialis",
    "crypto airdrop",
    "free bitcoin",
    "seo service",
    "backlinks",
    "buy followers",
    "payday loan",
    "escort",
    "porn",
    "work from home and earn",
    "click here",
    "whatsapp me",
    "telegram me",
    "guaranteed profit",
];

const SHORTENERS: &[&str] = &[
    "bit.ly",
    "tinyurl.com",
    "t.co/",
    "goo.gl",
    "is.gd",
    "cutt.ly",
    "rb.gy",
];

/// Returns a score (0 is clean, >= threshold is quarantined) and the triggered signals.
pub fn score(title: &str, body: &str) -> (f32, Vec<&'static str>) {
    let text = format!("{title}\n{body}");
    let lower = text.to_lowercase();
    let mut score = 0.0f32;
    let mut reasons = Vec::new();

    let words: Vec<&str> = lower.split_whitespace().collect();
    let links = words
        .iter()
        .filter(|w| w.contains("http://") || w.contains("https://") || w.starts_with("www."))
        .count();
    let link_chars: usize = words
        .iter()
        .filter(|w| w.contains("http") || w.starts_with("www."))
        .map(|w| w.len())
        .sum();

    if links >= 3 {
        score += 0.4;
        reasons.push("many_links");
    }
    if !lower.is_empty() && link_chars * 2 > lower.len() {
        score += 0.5;
        reasons.push("mostly_links");
    }
    if SHORTENERS.iter().any(|s| lower.contains(s)) {
        score += 0.2;
        reasons.push("url_shortener");
    }
    if lower.contains("<a href") || lower.contains("[url=") || lower.contains("[/url]") {
        score += 0.5;
        reasons.push("link_markup");
    }

    let phrase_hits = SPAM_PHRASES.iter().filter(|p| lower.contains(*p)).count();
    if phrase_hits > 0 {
        score += (0.35 * phrase_hits as f32).min(0.9);
        reasons.push("spam_phrases");
    }

    let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() > 20 {
        let upper = letters.iter().filter(|c| c.is_uppercase()).count();
        if upper * 10 > letters.len() * 7 {
            score += 0.25;
            reasons.push("shouting");
        }
    }

    if has_char_run(&text, 12) {
        score += 0.25;
        reasons.push("char_run");
    }

    if words.len() >= 30 {
        let mut unique = words.clone();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() * 5 < words.len() {
            score += 0.35;
            reasons.push("repetitive");
        }
    }

    (score, reasons)
}

fn has_char_run(text: &str, threshold: usize) -> bool {
    let mut last = None;
    let mut run = 0;
    for c in text.chars() {
        if c.is_whitespace() {
            last = None;
            run = 0;
            continue;
        }
        if Some(c) == last {
            run += 1;
            if run >= threshold {
                return true;
            }
        } else {
            last = Some(c);
            run = 1;
        }
    }
    false
}

/// Resolves the client IP. With `trusted_hops = N`, the Nth entry from the right of
/// `X-Forwarded-For` is used (each trusted proxy appends the address it saw).
/// Anything left of that is client controlled and ignored.
pub fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>, trusted_hops: usize) -> String {
    if trusted_hops > 0 {
        let chain: Vec<String> = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if chain.len() >= trusted_hops
            && let Ok(ip) = chain[chain.len() - trusted_hops].parse::<IpAddr>()
        {
            return normalize_ip(ip);
        }
    }
    peer.map(|p| normalize_ip(p.ip()))
        .unwrap_or_else(|| "unknown".into())
}

/// IPv6 clients usually own a whole /64, so rate limit at that granularity.
fn normalize_ip(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let s = v6.segments();
                format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_feedback_scores_zero() {
        let (s, r) = score(
            "Crash when opening inventory",
            "The game crashes every time I press I during the boss fight. Version 1.2.3 on Steam Deck.",
        );
        assert_eq!(s, 0.0, "{r:?}");
    }

    #[test]
    fn obvious_spam_crosses_threshold() {
        let (s, r) = score(
            "Best casino",
            "Visit https://a.example https://b.example https://bit.ly/x for free bitcoin",
        );
        assert!(s >= 0.7, "{s} {r:?}");
        assert!(r.contains(&"many_links"));
        assert!(r.contains(&"spam_phrases"));
    }

    #[test]
    fn detects_shouting_runs_and_repetition() {
        assert!(
            score("", "THIS GAME IS TERRIBLE AND YOU SHOULD FEEL BAD")
                .1
                .contains(&"shouting")
        );
        assert!(score("", "nooooooooooooooooo").1.contains(&"char_run"));
        let repeated = "spam ".repeat(40);
        assert!(score("", &repeated).1.contains(&"repetitive"));
    }

    #[test]
    fn content_hash_ignores_case_and_whitespace() {
        assert_eq!(content_hash("A", "b  c\n"), content_hash("a", "B c"));
        assert_ne!(content_hash("a", "b"), content_hash("a", "c"));
    }

    #[test]
    fn sanitize_strips_control_chars() {
        assert_eq!(sanitize("  hi\u{0}\u{7}\nthere\t "), "hi\nthere");
    }

    #[test]
    fn client_ip_respects_trusted_hops() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", "6.6.6.6, 1.2.3.4".parse().unwrap());
        let peer: SocketAddr = "10.0.0.1:5000".parse().unwrap();
        assert_eq!(client_ip(&h, Some(peer), 0), "10.0.0.1");
        assert_eq!(client_ip(&h, Some(peer), 1), "1.2.3.4");
        assert_eq!(client_ip(&h, Some(peer), 2), "6.6.6.6");
        assert_eq!(client_ip(&h, Some(peer), 3), "10.0.0.1");
        let mut v6 = HeaderMap::new();
        v6.insert("x-forwarded-for", "2001:db8:1:2:3:4:5:6".parse().unwrap());
        assert_eq!(client_ip(&v6, None, 1), "2001:db8:1:2::/64");
    }
}
