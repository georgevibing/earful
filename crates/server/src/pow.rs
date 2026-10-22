//! Stateless, signed proof-of-work challenges.
//!
//! Token format: `v1.<project_id>.<issued_ms>.<bits>.<id>.<sig>` where `sig` is a
//! truncated HMAC over everything before it. The client must find a decimal
//! `nonce` such that `sha256("<token>:<nonce>")` starts with `bits` zero bits.
//! Each `id` can be redeemed once (enforced by the caller via `used_challenges`).

use crate::crypto;

const SIG_BYTES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub project_id: i64,
    pub issued_ms: i64,
    pub bits: u32,
    pub id: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PowError {
    Malformed,
    BadSignature,
    WrongProject,
    Expired,
    InsufficientWork,
}

impl PowError {
    pub fn code(&self) -> &'static str {
        match self {
            PowError::Malformed => "challenge_malformed",
            PowError::BadSignature => "challenge_invalid",
            PowError::WrongProject => "challenge_wrong_project",
            PowError::Expired => "challenge_expired",
            PowError::InsufficientWork => "pow_insufficient",
        }
    }
}

pub fn issue(secret: &[u8], project_id: i64, issued_ms: i64, bits: u32) -> String {
    let id = crypto::random_hex(16);
    sign(secret, project_id, issued_ms, bits, &id)
}

pub fn sign(secret: &[u8], project_id: i64, issued_ms: i64, bits: u32, id: &str) -> String {
    let unsigned = format!("v1.{project_id}.{issued_ms}.{bits}.{id}");
    let sig = &crypto::hmac_hex(secret, unsigned.as_bytes())[..SIG_BYTES * 2];
    format!("{unsigned}.{sig}")
}

pub fn parse(secret: &[u8], token: &str) -> Result<Challenge, PowError> {
    if token.len() > 200 {
        return Err(PowError::Malformed);
    }
    let (unsigned, sig) = token.rsplit_once('.').ok_or(PowError::Malformed)?;
    let parts: Vec<&str> = unsigned.split('.').collect();
    let [version, pid, issued, bits, id] = parts.as_slice() else {
        return Err(PowError::Malformed);
    };
    if *version != "v1" || id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(PowError::Malformed);
    }
    if !crypto::hmac_verify_hex(secret, unsigned.as_bytes(), sig, SIG_BYTES) {
        return Err(PowError::BadSignature);
    }
    Ok(Challenge {
        project_id: pid.parse().map_err(|_| PowError::Malformed)?,
        issued_ms: issued.parse().map_err(|_| PowError::Malformed)?,
        bits: bits.parse().map_err(|_| PowError::Malformed)?,
        id: (*id).to_string(),
    })
}

/// Verifies signature, scope, freshness and work. Does not check single use.
pub fn verify(
    secret: &[u8],
    token: &str,
    nonce: &str,
    project_id: i64,
    now_ms: i64,
    ttl_secs: i64,
) -> Result<Challenge, PowError> {
    let challenge = parse(secret, token)?;
    if challenge.project_id != project_id {
        return Err(PowError::WrongProject);
    }
    let age_ms = now_ms - challenge.issued_ms;
    if age_ms > ttl_secs * 1000 || age_ms < -60_000 {
        return Err(PowError::Expired);
    }
    if nonce.is_empty() || nonce.len() > 20 || !nonce.bytes().all(|b| b.is_ascii_digit()) {
        return Err(PowError::InsufficientWork);
    }
    if work_bits(token, nonce) < challenge.bits {
        return Err(PowError::InsufficientWork);
    }
    Ok(challenge)
}

pub fn work_bits(token: &str, nonce: &str) -> u32 {
    crypto::leading_zero_bits(&crypto::sha256(format!("{token}:{nonce}").as_bytes()))
}

/// Reference solver, used by tests and the CLI.
pub fn solve(token: &str, bits: u32) -> String {
    (0u64..)
        .map(|n| n.to_string())
        .find(|n| work_bits(token, n) >= bits)
        .expect("search space is effectively unbounded")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-secret";

    #[test]
    fn round_trip() {
        let token = issue(SECRET, 7, 1_000_000, 8);
        let nonce = solve(&token, 8);
        let c = verify(SECRET, &token, &nonce, 7, 1_000_500, 60).unwrap();
        assert_eq!(c.project_id, 7);
        assert_eq!(c.bits, 8);
    }

    #[test]
    fn rejects_tampering_scope_expiry_and_weak_work() {
        let token = issue(SECRET, 7, 1_000_000, 8);
        let nonce = solve(&token, 8);
        let tampered = token.replacen("v1.7.", "v1.8.", 1);
        assert_eq!(verify(SECRET, &tampered, &nonce, 8, 1_000_000, 60), Err(PowError::BadSignature));
        assert_eq!(verify(SECRET, &token, &nonce, 9, 1_000_000, 60), Err(PowError::WrongProject));
        assert_eq!(verify(SECRET, &token, &nonce, 7, 1_000_000 + 61_000, 60), Err(PowError::Expired));
        assert_eq!(verify(b"other", &token, &nonce, 7, 1_000_000, 60), Err(PowError::BadSignature));
        assert_eq!(verify(SECRET, "garbage", &nonce, 7, 1_000_000, 60), Err(PowError::Malformed));
        assert_eq!(verify(SECRET, &token, "abc", 7, 1_000_000, 60), Err(PowError::InsufficientWork));
        let bad = (0u64..).map(|n| n.to_string()).find(|n| work_bits(&token, n) < 8).unwrap();
        assert_eq!(verify(SECRET, &token, &bad, 7, 1_000_000, 60), Err(PowError::InsufficientWork));
    }

    /// Shared vector with `packages/widget/src/pow.test.ts`. Keep both in sync.
    #[test]
    fn cross_language_vector() {
        let token = "v1.1.1700000000000.12.0123456789abcdef0123456789abcdef.deadbeef";
        assert_eq!(solve(token, 12), "1627");
        assert!(work_bits(token, "1627") >= 12);
    }
}
