//! Request limits by source address for the unauthenticated OAuth endpoints.
//!
//! Registration and Client ID Metadata Document fetches need no Oceans
//! credential, so the only thing to limit them by is where they come from.
//! Each limiter counts requests in a fixed one-minute window, per source and
//! across the gateway. Counts live in memory, so each replica enforces its own
//! limits.

use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

const WINDOW: Duration = Duration::from_secs(60);
/// Sources tracked in one window. Past this, new sources count only against
/// the gateway-wide limit, so many addresses cannot grow the map without bound.
const MAX_TRACKED_SOURCES: usize = 16_384;

/// Requests allowed per minute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRateLimits {
    pub per_source_per_minute: u32,
    pub per_minute: u32,
}

/// A request over a limit, and when the window resets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimited {
    pub retry_after: Duration,
}

impl RateLimited {
    /// Whole seconds until the window resets, rounded up so a client that
    /// waits that long is admitted.
    #[must_use]
    pub fn retry_after_seconds(&self) -> u64 {
        self.retry_after.as_secs() + u64::from(self.retry_after.subsec_nanos() > 0)
    }
}

#[derive(Debug)]
pub struct SourceRateLimiter {
    limits: SourceRateLimits,
    state: Mutex<WindowState>,
}

#[derive(Debug)]
struct WindowState {
    started: Option<Instant>,
    total: u32,
    by_source: HashMap<SourceKey, u32>,
}

/// IPv6 sources are counted by /64, the smallest block a host is usually
/// given, so one host cannot rotate through its own addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SourceKey {
    V4(u32),
    V6(u64),
}

impl From<IpAddr> for SourceKey {
    fn from(address: IpAddr) -> Self {
        match address {
            IpAddr::V4(v4) => Self::V4(u32::from(v4)),
            IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) => Self::V4(u32::from(v4)),
                None => Self::V6((u128::from(v6) >> 64) as u64),
            },
        }
    }
}

impl SourceRateLimiter {
    #[must_use]
    pub fn new(limits: SourceRateLimits) -> Self {
        Self {
            limits,
            state: Mutex::new(WindowState {
                started: None,
                total: 0,
                by_source: HashMap::new(),
            }),
        }
    }

    #[must_use]
    pub fn limits(&self) -> SourceRateLimits {
        self.limits
    }

    /// Counts one request from `source` (unknown when `None`) and refuses it
    /// when it would pass either limit. A refused request is not counted.
    pub fn check(&self, source: Option<IpAddr>, now: Instant) -> Result<(), RateLimited> {
        let mut state = self.state.lock().expect("source rate limiter lock");
        let started = match state.started {
            Some(started) if now.saturating_duration_since(started) < WINDOW => started,
            _ => {
                state.started = Some(now);
                state.total = 0;
                state.by_source.clear();
                now
            }
        };
        let limited = RateLimited {
            retry_after: WINDOW.saturating_sub(now.saturating_duration_since(started)),
        };
        if state.total >= self.limits.per_minute {
            return Err(limited);
        }
        if let Some(key) = source.map(SourceKey::from) {
            let tracked = state.by_source.len() < MAX_TRACKED_SOURCES;
            match state.by_source.get_mut(&key) {
                Some(count) if *count >= self.limits.per_source_per_minute => return Err(limited),
                Some(count) => *count += 1,
                None if tracked => {
                    state.by_source.insert(key, 1);
                }
                None => {}
            }
        }
        state.total += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn sources_are_limited_separately_and_together() {
        let limiter = SourceRateLimiter::new(SourceRateLimits {
            per_source_per_minute: 2,
            per_minute: 3,
        });
        let start = Instant::now();
        let a = Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)));
        let b = Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 2)));
        assert!(limiter.check(a, start).is_ok());
        assert!(limiter.check(a, start).is_ok());
        let refused = limiter
            .check(a, start + WINDOW / 2)
            .expect_err("per source");
        assert_eq!(refused.retry_after, WINDOW / 2);
        let fraction = RateLimited {
            retry_after: Duration::from_millis(30_900),
        };
        assert_eq!(fraction.retry_after_seconds(), 31);
        assert!(limiter.check(b, start).is_ok());
        assert!(limiter.check(None, start).is_err(), "gateway-wide");
        assert!(limiter.check(a, start + WINDOW).is_ok(), "next window");
    }

    #[test]
    fn ipv6_sources_share_a_slash_64() {
        let limiter = SourceRateLimiter::new(SourceRateLimits {
            per_source_per_minute: 1,
            per_minute: 100,
        });
        let now = Instant::now();
        let first = Ipv6Addr::new(0x2001, 0xdb8, 1, 2, 0, 0, 0, 1);
        let same_block = Ipv6Addr::new(0x2001, 0xdb8, 1, 2, 9, 9, 9, 9);
        let other_block = Ipv6Addr::new(0x2001, 0xdb8, 1, 3, 0, 0, 0, 1);
        assert!(limiter.check(Some(first.into()), now).is_ok());
        assert!(limiter.check(Some(same_block.into()), now).is_err());
        assert!(limiter.check(Some(other_block.into()), now).is_ok());
    }
}
