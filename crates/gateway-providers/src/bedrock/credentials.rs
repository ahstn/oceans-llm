use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use aws_credential_types::{
    Credentials,
    provider::{ProvideCredentials, error::CredentialsError},
};
use thiserror::Error;
use tokio::sync::Mutex;

const REFRESH_MARGIN: Duration = Duration::from_secs(60);
const UNDATED_CREDENTIAL_TTL: Duration = Duration::from_secs(15 * 60);
// Let queued callers share a failed refresh instead of serially retrying STS.
const FAILURE_RETRY_DELAY: Duration = Duration::from_secs(1);

/// The default chain resolves credentials but does not cache STS results.
/// Keep the cache with the provider instance so routes and clones share refreshes.
pub(super) struct CachedCredentials<P> {
    provider: P,
    cached: Mutex<Option<CachedEntry>>,
}

struct CachedEntry {
    result: Result<Credentials, CredentialCacheError>,
    refresh_at: SystemTime,
}

#[derive(Debug, Error, Clone)]
pub(super) enum CredentialCacheError {
    #[error("failed to resolve aws_bedrock default credentials: {0}")]
    Resolve(#[source] Arc<CredentialsError>),
    #[error("aws_bedrock credential provider returned expired credentials")]
    Expired,
}

impl<P: ProvideCredentials> CachedCredentials<P> {
    pub(super) fn new(provider: P) -> Self {
        Self {
            provider,
            cached: Mutex::new(None),
        }
    }

    pub(super) async fn credentials(&self) -> Result<Credentials, CredentialCacheError> {
        self.credentials_with_clock(SystemTime::now).await
    }

    async fn credentials_with_clock(
        &self,
        now: impl Fn() -> SystemTime,
    ) -> Result<Credentials, CredentialCacheError> {
        // Hold the lock through refresh. Waiting callers recheck the completed
        // cache entry, including after cancellation or a failed refresh.
        let mut cached = self.cached.lock().await;
        if let Some(entry) = cached.as_ref()
            && now() < entry.refresh_at
        {
            // Both credentials and source errors use shared handles. Signing
            // needs an owned handle, not another copy of the secret strings.
            return entry.result.clone();
        }
        let result = self
            .provider
            .provide_credentials()
            .await
            .map_err(|error| CredentialCacheError::Resolve(Arc::new(error)));
        let now = now();
        let entry = match result.and_then(|credentials| CachedEntry::fresh(credentials, now)) {
            Ok(entry) => entry,
            Err(error) => {
                // An early refresh failure does not invalidate an identity.
                // Bound the retry by its actual expiry so cached fallback can
                // never extend its lifetime, even by part of a second.
                if let Some(previous) = cached.as_mut()
                    && let Ok(credentials) = &previous.result
                    && let Some(expiry) = credentials.expiry()
                    && now < expiry
                {
                    previous.refresh_at = (now + FAILURE_RETRY_DELAY).min(expiry);
                    return previous.result.clone();
                }
                CachedEntry {
                    result: Err(error),
                    refresh_at: now + FAILURE_RETRY_DELAY,
                }
            }
        };
        let result = entry.result.clone();
        *cached = Some(entry);
        result
    }
}

impl CachedEntry {
    fn fresh(credentials: Credentials, now: SystemTime) -> Result<Self, CredentialCacheError> {
        let refresh_at = match credentials.expiry() {
            Some(expiry) => {
                let remaining = expiry
                    .duration_since(now)
                    .ok()
                    .filter(|ttl| !ttl.is_zero())
                    .ok_or(CredentialCacheError::Expired)?;
                // Scale the margin for short-lived credentials to avoid an
                // immediate refresh loop when the remaining TTL is under 60s.
                expiry - REFRESH_MARGIN.min(remaining / 5)
            }
            // Periodically re-resolve sources that omit expiry, such as a
            // credential process, so their rotated credentials are discovered.
            None => now + UNDATED_CREDENTIAL_TTL,
        };
        Ok(Self {
            result: Ok(credentials),
            refresh_at,
        })
    }
}

#[cfg(test)]
#[path = "credentials_tests.rs"]
mod tests;
