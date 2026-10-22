//! Small hashing helpers shared by sessions, challenges and IP pseudonymisation.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(sha256(data))
}

pub fn hmac_hex(key: &[u8], data: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(data);
    hex::encode(mac.finalize().into_bytes())
}

/// Constant time comparison of an expected HMAC (hex) against a provided one.
pub fn hmac_verify_hex(key: &[u8], data: &[u8], provided_hex: &str, truncate_bytes: usize) -> bool {
    let Ok(provided) = hex::decode(provided_hex) else {
        return false;
    };
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(data);
    mac.verify_truncated_left(&provided).is_ok() && provided.len() == truncate_bytes
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::fill(buf.as_mut_slice());
    hex::encode(buf)
}

pub fn leading_zero_bits(hash: &[u8]) -> u32 {
    let mut bits = 0;
    for byte in hash {
        if *byte == 0 {
            bits += 8;
        } else {
            bits += byte.leading_zeros();
            break;
        }
    }
    bits
}

/// Pseudonymises an IP so raw addresses never touch the database.
pub fn ip_hash(secret: &[u8], ip: &str) -> String {
    hmac_hex(secret, format!("ip:{ip}").as_bytes())[..32].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_leading_zero_bits() {
        assert_eq!(leading_zero_bits(&[0xff]), 0);
        assert_eq!(leading_zero_bits(&[0x00, 0x80]), 8);
        assert_eq!(leading_zero_bits(&[0x00, 0x0f]), 12);
        assert_eq!(leading_zero_bits(&[0x00, 0x00]), 16);
    }

    #[test]
    fn hmac_verify_round_trips() {
        let sig = &hmac_hex(b"k", b"data")[..32];
        assert!(hmac_verify_hex(b"k", b"data", sig, 16));
        assert!(!hmac_verify_hex(b"k", b"other", sig, 16));
        assert!(!hmac_verify_hex(b"k", b"data", "zz", 16));
        assert!(!hmac_verify_hex(b"k", b"data", &sig[..30], 16));
    }

    #[test]
    fn ip_hash_is_stable_and_keyed() {
        assert_eq!(ip_hash(b"a", "1.2.3.4"), ip_hash(b"a", "1.2.3.4"));
        assert_ne!(ip_hash(b"a", "1.2.3.4"), ip_hash(b"b", "1.2.3.4"));
        assert_eq!(ip_hash(b"a", "1.2.3.4").len(), 32);
    }
}
