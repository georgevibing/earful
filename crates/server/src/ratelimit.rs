//! Sliding window rate limiting backed by the `rate_events` table.

use rusqlite::{Connection, params};

pub struct Rule<'a> {
    pub bucket: &'a str,
    /// `None` counts events from every IP (a global cap for the bucket).
    pub ip_hash: Option<&'a str>,
    pub window_secs: i64,
    pub limit: u32,
}

/// Returns `Err(retry_after_secs)` for the first rule that is exhausted.
pub fn check(conn: &Connection, now: i64, rules: &[Rule]) -> rusqlite::Result<Result<(), i64>> {
    for rule in rules {
        let since = now - rule.window_secs;
        let (count, oldest): (u32, Option<i64>) = conn.query_row(
            "SELECT COUNT(*), MIN(ts) FROM rate_events
             WHERE bucket = ?1 AND (?2 IS NULL OR ip_hash = ?2) AND ts > ?3",
            params![rule.bucket, rule.ip_hash, since],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if count >= rule.limit {
            let retry = oldest
                .map(|o| o + rule.window_secs - now)
                .unwrap_or(rule.window_secs);
            return Ok(Err(retry.max(1)));
        }
    }
    Ok(Ok(()))
}

pub fn record(conn: &Connection, bucket: &str, ip_hash: &str, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO rate_events (bucket, ip_hash, ts) VALUES (?1, ?2, ?3)",
        params![bucket, ip_hash, now],
    )?;
    Ok(())
}

pub fn count(
    conn: &Connection,
    bucket: &str,
    ip_hash: Option<&str>,
    since: i64,
) -> rusqlite::Result<u32> {
    conn.query_row(
        "SELECT COUNT(*) FROM rate_events WHERE bucket = ?1 AND (?2 IS NULL OR ip_hash = ?2) AND ts > ?3",
        params![bucket, ip_hash, since],
        |r| r.get(0),
    )
}

pub fn record_block(
    conn: &Connection,
    project_id: Option<i64>,
    reason: &str,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO block_events (project_id, reason, ts) VALUES (?1, ?2, ?3)",
        params![project_id, reason, now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut c).unwrap();
        c
    }

    #[test]
    fn enforces_window_and_reports_retry_after() {
        let c = conn();
        let rule = [Rule {
            bucket: "b",
            ip_hash: Some("ip"),
            window_secs: 60,
            limit: 2,
        }];
        assert_eq!(check(&c, 1000, &rule).unwrap(), Ok(()));
        record(&c, "b", "ip", 1000).unwrap();
        record(&c, "b", "ip", 1010).unwrap();
        assert_eq!(check(&c, 1020, &rule).unwrap(), Err(40));
        // Other IPs are unaffected, and the window slides.
        let other = [Rule {
            ip_hash: Some("other"),
            ..rule[0]
        }];
        assert_eq!(check(&c, 1020, &other).unwrap(), Ok(()));
        assert_eq!(check(&c, 1061, &rule).unwrap(), Ok(()));
    }

    #[test]
    fn global_rule_counts_all_ips() {
        let c = conn();
        record(&c, "b", "a", 100).unwrap();
        record(&c, "b", "b", 100).unwrap();
        let rule = [Rule {
            bucket: "b",
            ip_hash: None,
            window_secs: 60,
            limit: 2,
        }];
        assert!(check(&c, 110, &rule).unwrap().is_err());
    }
}
