//! Client address resolution and the signed record of it for admin UI requests.
//!
//! [`resolve_client_ip`] finds the client behind configured trusted proxies.
//! The socket peer is the client unless it is a trusted proxy; only then is
//! `X-Forwarded-For` read, from the right, skipping trusted hops, so a caller
//! cannot pick its own address by sending the header directly. The gateway
//! uses it for request policy and audit entries.
//!
//! The gateway's admin UI proxy is the edge the browser's connection reaches.
//! It resolves the client this way and signs the address into
//! [`CLIENT_IP_HEADER`]; the admin UI server forwards the header on its gateway
//! API calls, and the gateway trusts it only when the signature is valid and
//! fresh.

use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use axum::http::HeaderMap;
use hmac::{Hmac, Mac};
use ipnet::IpNet;
use sha2::Sha256;

pub const CLIENT_IP_HEADER: &str = "x-oceans-client-ip";

/// How long a signed address stays valid, in seconds, either side of `now`.
const MAX_AGE_SECS: i64 = 300;
const DOMAIN: &[u8] = b"oceans-admin-client-ip:";

type HmacSha256 = Hmac<Sha256>;

const X_FORWARDED_FOR: &str = "x-forwarded-for";

#[derive(Debug, Clone, Default)]
pub struct TrustedProxies(Arc<Vec<IpNet>>);

impl TrustedProxies {
    #[must_use]
    pub fn new(networks: Vec<IpNet>) -> Self {
        Self(Arc::new(networks))
    }

    fn contains(&self, ip: IpAddr) -> bool {
        self.0.iter().any(|network| network.contains(&ip))
    }
}

/// The client's IP, or `None` when the socket peer is unknown (for example,
/// in-process tests that do not record connection info).
#[must_use]
pub fn resolve_client_ip(
    peer: Option<IpAddr>,
    headers: &HeaderMap,
    trusted: &TrustedProxies,
) -> Option<IpAddr> {
    let mut client = peer?.to_canonical();
    if !trusted.contains(client) {
        return Some(client);
    }
    let hops: Vec<&str> = headers
        .get_all(X_FORWARDED_FOR)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .collect();
    for hop in hops.into_iter().rev() {
        // A malformed entry ends the walk at the last trusted hop.
        let Some(ip) = parse_hop(hop) else {
            break;
        };
        client = ip;
        if !trusted.contains(ip) {
            break;
        }
    }
    Some(client)
}

fn parse_hop(hop: &str) -> Option<IpAddr> {
    let hop = hop.trim();
    hop.parse::<IpAddr>()
        .or_else(|_| hop.parse::<SocketAddr>().map(|address| address.ip()))
        .ok()
        .map(|ip| ip.to_canonical())
}

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
    // `abs_diff` cannot overflow, whatever timestamp an unauthenticated client sends.
    if now_unix_secs.abs_diff(issued_at) > MAX_AGE_SECS.unsigned_abs() {
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
    use axum::http::HeaderValue;

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
        let extreme = format!("198.51.100.1;{};00", i64::MIN);
        assert_eq!(verify_client_ip(KEY, &extreme, NOW), None);
        assert_eq!(verify_client_ip(KEY, &extreme, i64::MAX), None);
    }

    fn trusted(entries: &[&str]) -> TrustedProxies {
        TrustedProxies::new(entries.iter().map(|entry| entry.parse().unwrap()).collect())
    }

    fn forwarded(values: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(X_FORWARDED_FOR, HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    fn ip(value: &str) -> IpAddr {
        value.parse().unwrap()
    }

    #[test]
    fn an_untrusted_peer_is_the_client_whatever_the_header_says() {
        let headers = forwarded(&["203.0.113.9"]);
        assert_eq!(
            resolve_client_ip(
                Some(ip("198.51.100.4")),
                &headers,
                &trusted(&["10.0.0.0/8"])
            ),
            Some(ip("198.51.100.4"))
        );
        assert_eq!(
            resolve_client_ip(Some(ip("10.0.0.2")), &headers, &TrustedProxies::default()),
            Some(ip("10.0.0.2"))
        );
        assert_eq!(
            resolve_client_ip(None, &headers, &trusted(&["10.0.0.0/8"])),
            None
        );
    }

    #[test]
    fn trusted_hops_are_skipped_from_the_right() {
        let proxies = trusted(&["10.0.0.0/8", "fd00::/8"]);
        // The left-most entry is caller-controlled and is never reached.
        let headers = forwarded(&["1.1.1.1, 203.0.113.9", "10.1.2.3"]);
        assert_eq!(
            resolve_client_ip(Some(ip("10.0.0.2")), &headers, &proxies),
            Some(ip("203.0.113.9"))
        );

        let v6 = forwarded(&["2001:db8::7, fd00::1"]);
        assert_eq!(
            resolve_client_ip(Some(ip("fd00::2")), &v6, &proxies),
            Some(ip("2001:db8::7"))
        );
    }

    #[test]
    fn mapped_addresses_ports_and_bad_entries_are_handled() {
        let proxies = trusted(&["10.0.0.0/8"]);
        // An IPv4-mapped IPv6 peer is matched as IPv4.
        let headers = forwarded(&["203.0.113.9:4711"]);
        assert_eq!(
            resolve_client_ip(Some(ip("::ffff:10.0.0.2")), &headers, &proxies),
            Some(ip("203.0.113.9"))
        );
        // A malformed hop stops at the nearest trusted proxy.
        let bad = forwarded(&["203.0.113.9, not-an-ip, 10.0.0.9"]);
        assert_eq!(
            resolve_client_ip(Some(ip("10.0.0.2")), &bad, &proxies),
            Some(ip("10.0.0.9"))
        );
        // No header: the trusted proxy itself is all we know.
        assert_eq!(
            resolve_client_ip(Some(ip("10.0.0.2")), &HeaderMap::new(), &proxies),
            Some(ip("10.0.0.2"))
        );
    }
}
