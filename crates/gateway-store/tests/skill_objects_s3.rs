//! Run explicitly against a provisioned RustFS or S3 test bucket. Tests create
//! and delete only a random object prefix, never the bucket or other objects.

use std::{env, time::Duration};

use gateway_core::{SkillObjectStore, StoreError};
use gateway_store::{S3SkillObjectStore, S3SkillStorageConfig};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires SKILL_S3_TEST_ENDPOINT and SKILL_S3_TEST_BUCKET plus AWS credentials"]
async fn s3_archive_lifecycle() {
    let endpoint = env::var("SKILL_S3_TEST_ENDPOINT").expect("SKILL_S3_TEST_ENDPOINT must be set");
    let bucket = env::var("SKILL_S3_TEST_BUCKET").expect("SKILL_S3_TEST_BUCKET must be set");
    let store = S3SkillObjectStore::new(S3SkillStorageConfig {
        bucket,
        region: env::var("SKILL_S3_TEST_REGION").unwrap_or_else(|_| "us-east-1".to_owned()),
        allow_http: endpoint.starts_with("http://"),
        endpoint: Some(endpoint),
        prefix: format!("oceans-contract-test/{}", Uuid::new_v4()),
        force_path_style: true,
        access_key_id: env::var("SKILL_S3_TEST_ACCESS_KEY_ID").ok(),
        secret_access_key: env::var("SKILL_S3_TEST_SECRET_ACCESS_KEY").ok(),
        session_token: env::var("SKILL_S3_TEST_SESSION_TOKEN").ok(),
        max_object_bytes: 1024,
        request_timeout: Duration::from_secs(30),
    })
    .await
    .expect("construct test object store");

    let archive = b"skill storage contract fixture";
    store
        .put("archive.zip", archive)
        .await
        .expect("upload object");
    let actual = store.get("archive.zip").await;
    let replacement = store.put("archive.zip", b"must not replace original").await;
    let original = store.get("archive.zip").await;
    // Clean up before assertions, including when a read or immutability check fails.
    store
        .delete("archive.zip")
        .await
        .expect("delete test object");
    assert_eq!(actual.expect("download object"), archive);
    assert!(matches!(replacement, Err(StoreError::Conflict(_))));
    assert_eq!(original.expect("download original object"), archive);
    assert!(matches!(
        store.get("archive.zip").await,
        Err(StoreError::NotFound(_))
    ));
    store
        .delete("archive.zip")
        .await
        .expect("delete remains idempotent");
}
