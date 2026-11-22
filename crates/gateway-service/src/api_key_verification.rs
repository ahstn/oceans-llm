use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, LazyLock, RwLock},
    time::Duration,
};

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordVerifier},
};
use gateway_core::{AuthError, GatewayError};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;

use crate::credential_verification::{BlockingCredentialError, constant_time_eq, run_limited};

/// Caps retained successful verifications, including hashes for rotated keys.
const MAX_VERIFICATION_MEMO_ENTRIES: usize = 4096;
/// Bounds the CPU and memory used by cold API-key verification.
const MAX_CONCURRENT_ARGON2: usize = 4;
/// Matches the admission wait used for legacy OAuth client secrets.
const ARGON2_SLOT_WAIT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct VerificationMemo {
    entries: HashMap<Arc<str>, [u8; 32]>,
    insertion_order: VecDeque<Arc<str>>,
}

impl VerificationMemo {
    fn remember(&mut self, hash: &str, digest: [u8; 32]) {
        // Concurrent cold requests can finish after the first request cached this hash.
        if self.entries.contains_key(hash) {
            return;
        }
        if self.entries.len() == MAX_VERIFICATION_MEMO_ENTRIES
            && let Some(oldest) = self.insertion_order.pop_front()
        {
            self.entries.remove(&oldest);
        }
        let hash: Arc<str> = Arc::from(hash);
        self.insertion_order.push_back(Arc::clone(&hash));
        self.entries.insert(hash, digest);
    }
}

#[derive(Default)]
pub(crate) struct ApiKeyVerification {
    memo: RwLock<VerificationMemo>,
    #[cfg(test)]
    argon2_verifications: std::sync::atomic::AtomicUsize,
}

impl ApiKeyVerification {
    pub(crate) async fn verify(&self, secret: &str, hash: &str) -> Result<(), GatewayError> {
        let digest: [u8; 32] = Sha256::digest(secret.as_bytes()).into();
        {
            let memo = self
                .memo
                .read()
                .expect("API key verification memo lock poisoned");
            if let Some(expected) = memo.entries.get(hash) {
                return if constant_time_eq(expected, &digest) {
                    Ok(())
                } else {
                    Err(AuthError::ApiKeySecretMismatch.into())
                };
            }
        }

        static SLOTS: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(MAX_CONCURRENT_ARGON2));
        let owned_secret = secret.to_owned();
        let owned_hash = hash.to_owned();
        #[cfg(test)]
        self.argon2_verifications
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        run_limited(&SLOTS, ARGON2_SLOT_WAIT, move || {
            let password_hash = PasswordHash::new(&owned_hash)
                .map_err(|error| AuthError::HashVerification(error.to_string()))?;
            Argon2::default()
                .verify_password(owned_secret.as_bytes(), &password_hash)
                .map_err(|_| AuthError::ApiKeySecretMismatch)
        })
        .await
        .map_err(|error| match error {
            BlockingCredentialError::Busy => {
                GatewayError::RateLimited("API key authentication is busy; retry later".to_string())
            }
            BlockingCredentialError::Unavailable => {
                GatewayError::Internal("credential hashing is unavailable".to_string())
            }
            BlockingCredentialError::Failed => {
                GatewayError::Internal("credential hashing failed".to_string())
            }
        })??;

        self.memo
            .write()
            .expect("API key verification memo lock poisoned")
            .remember(hash, digest);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use gateway_core::hash_gateway_key_secret;

    use super::*;

    #[tokio::test]
    async fn memo_hit_skips_argon2_and_rejects_the_wrong_secret() {
        let verifier = ApiKeyVerification::default();
        let hash = hash_gateway_key_secret("valid secret").expect("hash valid secret");
        verifier
            .verify("valid secret", &hash)
            .await
            .expect("cold authentication");
        verifier
            .verify("valid secret", &hash)
            .await
            .expect("memo authentication");
        assert!(matches!(
            verifier.verify("wrong secret", &hash).await,
            Err(GatewayError::Auth(AuthError::ApiKeySecretMismatch))
        ));
        assert_eq!(verifier.argon2_verifications.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn rotation_reverifies_and_failures_do_not_enter_the_memo() {
        let verifier = ApiKeyVerification::default();
        let first_hash = hash_gateway_key_secret("first secret").expect("hash first secret");
        let rotated_hash = hash_gateway_key_secret("rotated secret").expect("hash rotated secret");
        verifier
            .verify("first secret", &first_hash)
            .await
            .expect("first authentication");
        assert!(
            verifier
                .verify("first secret", &rotated_hash)
                .await
                .is_err()
        );
        assert!(
            !verifier
                .memo
                .read()
                .expect("memo lock")
                .entries
                .contains_key(rotated_hash.as_str())
        );
        verifier
            .verify("rotated secret", &rotated_hash)
            .await
            .expect("rotated authentication");
        verifier
            .verify("rotated secret", &rotated_hash)
            .await
            .expect("rotated memo authentication");
        assert_eq!(verifier.argon2_verifications.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn memo_evicts_the_oldest_success_without_growing() {
        let mut memo = VerificationMemo::default();
        for index in 0..=MAX_VERIFICATION_MEMO_ENTRIES {
            memo.remember(&index.to_string(), [0; 32]);
        }
        assert_eq!(memo.entries.len(), MAX_VERIFICATION_MEMO_ENTRIES);
        assert!(!memo.entries.contains_key("0"));
        assert!(
            memo.entries
                .contains_key(&MAX_VERIFICATION_MEMO_ENTRIES.to_string()[..])
        );
    }
}
