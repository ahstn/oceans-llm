use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

#[derive(Default)]
struct CleanupStore {
    deleted: AtomicUsize,
}

#[async_trait::async_trait]
impl SkillObjectStore for CleanupStore {
    async fn put(&self, _key: &str, _bytes: &[u8]) -> Result<(), StoreError> {
        unreachable!("cleanup does not upload")
    }

    async fn get(&self, _key: &str) -> Result<Vec<u8>, StoreError> {
        unreachable!("cleanup does not download")
    }

    async fn delete(&self, _key: &str) -> Result<(), StoreError> {
        self.deleted.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[tokio::test]
async fn metadata_rejection_cleans_up_but_uncertain_commit_preserves_archive() {
    let objects = CleanupStore::default();
    for error in [
        StoreError::Conflict("duplicate".into()),
        StoreError::NotFound("owner".into()),
    ] {
        assert!(
            finish_upload(&objects, "upload.zip", Err(error))
                .await
                .is_err()
        );
    }
    assert_eq!(objects.deleted.load(Ordering::Relaxed), 2);
    for error in [
        StoreError::Query("connection lost during commit".into()),
        StoreError::Unavailable("database unavailable".into()),
        StoreError::Unexpected("unknown outcome".into()),
    ] {
        assert!(
            finish_upload(&objects, "upload.zip", Err(error))
                .await
                .is_err()
        );
    }
    assert_eq!(objects.deleted.load(Ordering::Relaxed), 2);
}
