//! A signed record of the browser's address for one admin UI request.
//!
//! The gateway's admin UI proxy is the edge the browser connects to, so it is
//! the only hop that observes the real client address. It signs that address
//! into [`CLIENT_IP_HEADER`]; the admin UI server forwards the header on its
//! gateway API calls, and the gateway trusts it only when the signature is
//! valid and fresh. `X-Forwarded-For` is never trusted, so a client cannot
//! choose the address its audit entries record.

use std::net::IpAddr;

use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const CLIENT_IP_HEADER: &str = "x-oceans-client-ip";

/// How long a signed address stays valid, in seconds, either side of `now`.
const MAX_AGE_SECS: i64 = 300;
const DOMAIN: &[u8] = b"oceans-admin-client-ip:";

type HmacSha256 = Hmac<Sha256>;

/// Returns `<ip>;<unix seconds>;<hex signature>`.
#[must_use]
pub fn sign_client_ip(key: &[u8], ip: IpAddr, now_unix_secs: i64) -> String {
    let payload = format!("{ip};{now_unix_secs}");
    format!("{payload};{}", hex(&mac(key, &payload)))
}

/// Returns the address when the value carries a valid, unexpired signature.
#[must_use]
pub fn verify_client_ip(key: &[u8], value: &str, now_unix_secs: i64) -> Option<IpAddr> {
    let (payload, signature) = value.trim().rsplit_once(';')?;
    let (ip, issued_at) = payload.split_once(';')?;
    let ip: IpAddr = ip.parse().ok()?;
    let issued_at: i64 = issued_at.parse().ok()?;
    if (now_unix_secs - issued_at).abs() > MAX_AGE_SECS {
        return None;
    }
    let signature = unhex(signature)?;
    let mut verifier = HmacSha256::new_from_slice(key).ok()?;
    verifier.update(DOMAIN);
    verifier.update(payload.as_bytes());
    verifier.verify_slice(&signature).ok()?;
    Some(ip)
}

fn mac(key: &[u8], payload: &str) -> Vec<u8> {
    let mut signer = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    signer.update(DOMAIN);
    signer.update(payload.as_bytes());
    signer.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(text.get(index..index + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &[u8] = b"test-secret";
    const NOW: i64 = 1_800_000_000;

    #[test]
    fn signed_addresses_round_trip() {
        let ip: IpAddr = "203.0.113.9".parse().unwrap();
        let value = sign_client_ip(KEY, ip, NOW);
        assert_eq!(verify_client_ip(KEY, &value, NOW + 10), Some(ip));
        let v6: IpAddr = "2001:db8::7".parse().unwrap();
        assert_eq!(
            verify_client_ip(KEY, &sign_client_ip(KEY, v6, NOW), NOW),
            Some(v6)
        );
    }

    #[test]
    fn forged_tampered_and_stale_values_are_rejected() {
        let value = sign_client_ip(KEY, "203.0.113.9".parse().unwrap(), NOW);
        assert_eq!(verify_client_ip(b"other-secret", &value, NOW), None);
        let tampered = value.replacen("203.0.113.9", "198.51.100.1", 1);
        assert_eq!(verify_client_ip(KEY, &tampered, NOW), None);
        assert_eq!(verify_client_ip(KEY, &value, NOW + MAX_AGE_SECS + 1), None);
        assert_eq!(verify_client_ip(KEY, "198.51.100.1", NOW), None);
        assert_eq!(
            verify_client_ip(KEY, "198.51.100.1;1800000000;zz", NOW),
            None
        );
    }
}
