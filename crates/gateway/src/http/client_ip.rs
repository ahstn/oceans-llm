//! Client IP resolution through trusted proxies.
//!
//! The socket peer is the client unless it is a configured trusted proxy. Only
//! then is `X-Forwarded-For` read, from the right, skipping trusted hops, so a
//! caller cannot pick its own address by sending the header directly.

use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use axum::http::HeaderMap;
use ipnet::IpNet;

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

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

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
