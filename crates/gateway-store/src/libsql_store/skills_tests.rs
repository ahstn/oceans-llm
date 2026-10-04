use std::{sync::Arc, time::Duration};

use gateway_core::skills::{
    SkillManifest, SkillRepository, SkillVersionMetadata, SkillVersionRecord,
};
use gateway_core::{AuthMode, GlobalRole, StoreError, UserStatus};
use tempfile::{TempDir, tempdir};
use time::OffsetDateTime;
use tokio::sync::{Barrier, oneshot};
use uuid::Uuid;

use super::LibsqlStore;
use crate::run_migrations;

#[tokio::test]
async fn memory_connections_share_only_their_own_store() {
    let store = LibsqlStore::new_local(":memory:")
        .await
        .expect("memory store");
    store
        .connection()
        .execute("CREATE TABLE connection_probe (value TEXT)", ())
        .await
        .expect("create table on main connection");
    let connection = store.skill_connection().await.expect("skill connection");
    connection
        .execute("INSERT INTO connection_probe VALUES ('shared')", ())
        .await
        .expect("skill connection shares the memory schema");
    let mut rows = store
        .connection()
        .query("SELECT value FROM connection_probe", ())
        .await
        .expect("query shared memory schema");
    assert_eq!(
        rows.next()
            .await
            .expect("next")
            .expect("row")
            .get::<String>(0)
            .expect("value"),
        "shared"
    );
    let other = LibsqlStore::new_local(":memory:")
        .await
        .expect("other memory store");
    assert!(
        other
            .connection()
            .query("SELECT value FROM connection_probe", ())
            .await
            .is_err()
    );
}

async fn fixture() -> (TempDir, LibsqlStore, Uuid) {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("skills.db");
    run_migrations(&path).await.expect("migrations");
    let store = LibsqlStore::new_local(path.to_str().expect("database path"))
        .await
        .expect("store");
    let owner = store
        .create_identity_user(
            "Owner",
            "owner@example.com",
            "owner@example.com",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("owner");
    (directory, store, owner.user_id)
}

fn metadata(description: &str) -> SkillVersionMetadata {
    SkillVersionMetadata {
        description: description.to_string(),
        sha256: "a".repeat(64),
        object_key: format!("skills/{}.zip", Uuid::new_v4()),
        archive_bytes: 100,
        extracted_bytes: 200,
        file_count: 1,
        manifest: SkillManifest {
            name: "review".to_string(),
            description: description.to_string(),
            license: None,
            compatibility: None,
            metadata: Default::default(),
            allowed_tools: None,
            extra: Default::default(),
        },
        files: vec![gateway_core::skills::SkillFile {
            path: "SKILL.md".to_string(),
            size: 200,
        }],
        instructions: format!(
            "---\nname: review\ndescription: {description}\n---\nReview changes."
        ),
    }
}

async fn create_review_skill(store: &LibsqlStore, owner_id: Uuid) -> Uuid {
    let now = OffsetDateTime::now_utc();
    store
        .claim_skill_namespace(owner_id, "owner", now)
        .await
        .expect("namespace");
    store
        .create_skill(owner_id, "review", &metadata("original"), now)
        .await
        .expect("skill")
        .detail
        .skill
        .id
}

async fn session_fixture() -> (TempDir, LibsqlStore, Uuid, Uuid, OffsetDateTime) {
    let (directory, store, owner_id) = fixture().await;
    let skill_id = create_review_skill(&store, owner_id).await;
    let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("timestamp");
    store
        .append_skill_version(owner_id, skill_id, &metadata("updated"), now)
        .await
        .expect("second version");
    let session_id = Uuid::new_v4();
    store
        .create_user_session(
            session_id,
            owner_id,
            "session-hash",
            now + time::Duration::hours(1),
            now,
        )
        .await
        .expect("session");
    (directory, store, skill_id, session_id, now)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_touch_waits_for_skill_write_then_persists() {
    let (_directory, store, skill_id, session_id, created_at) = session_fixture().await;
    let connection = store.skill_connection().await.expect("skill connection");
    let transaction = connection
        .transaction_with_behavior(libsql::TransactionBehavior::Immediate)
        .await
        .expect("skill write transaction");
    transaction
        .execute(
            "UPDATE skills SET default_version = 2, description = 'updated' WHERE skill_id = ?1",
            [skill_id.to_string()],
        )
        .await
        .expect("hold skill write lock");

    let touched_at = created_at + time::Duration::seconds(30);
    let session_store = store.clone();
    let runtime = tokio::runtime::Handle::current();
    let (started, ready) = oneshot::channel();
    // Local libSQL waits synchronously, so keep the runtime free to release the writer.
    let mut touch = tokio::task::spawn_blocking(move || {
        started.send(()).expect("signal session writer");
        runtime.block_on(session_store.touch_user_session(session_id, touched_at))
    });
    ready.await.expect("session writer started");
    let early = tokio::time::timeout(Duration::from_millis(150), &mut touch).await;
    transaction.commit().await.expect("commit skill default");
    assert!(
        early.is_err(),
        "session touch must wait for the skill writer, not finish with a lock error: {early:?}"
    );
    tokio::time::timeout(Duration::from_secs(5), touch)
        .await
        .expect("session touch completes after skill commit")
        .expect("session writer task")
        .expect("session touch succeeds");

    let session = store
        .get_user_session(session_id)
        .await
        .expect("session")
        .expect("exists");
    assert_eq!(session.last_seen_at, touched_at);
    let skill = store
        .get_skill(skill_id)
        .await
        .expect("skill")
        .expect("exists");
    assert_eq!(skill.default_version, 2);
    assert_eq!(skill.description, "updated");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_touch_waits_for_skill_read_then_persists() {
    let (_directory, store, skill_id, session_id, created_at) = session_fixture().await;
    let connection = store.skill_connection().await.expect("skill connection");
    let transaction = connection
        .transaction()
        .await
        .expect("skill read transaction");
    let mut rows = transaction
        .query(
            "SELECT default_version FROM skills WHERE skill_id = ?1",
            [skill_id.to_string()],
        )
        .await
        .expect("establish skill read snapshot");
    let row = rows.next().await.expect("next").expect("skill");
    assert_eq!(row.get::<i64>(0).expect("default version"), 1);
    drop(row);
    rows.next().await.expect("finish skill read");
    drop(rows);

    let touched_at = created_at + time::Duration::seconds(30);
    let session_store = store.clone();
    let runtime = tokio::runtime::Handle::current();
    let (started, ready) = oneshot::channel();
    let mut touch = tokio::task::spawn_blocking(move || {
        started.send(()).expect("signal session writer");
        runtime.block_on(session_store.touch_user_session(session_id, touched_at))
    });
    ready.await.expect("session writer started");
    let early = tokio::time::timeout(Duration::from_millis(150), &mut touch).await;
    transaction.rollback().await.expect("release read snapshot");
    assert!(
        early.is_err(),
        "session touch must wait for the skill reader, not finish with a lock error: {early:?}"
    );
    tokio::time::timeout(Duration::from_secs(5), touch)
        .await
        .expect("session touch completes after skill read ends")
        .expect("session writer task")
        .expect("session touch succeeds");

    let session = store
        .get_user_session(session_id)
        .await
        .expect("session")
        .expect("exists");
    assert_eq!(session.last_seen_at, touched_at);
}

#[tokio::test]
async fn namespace_claim_survives_main_connection_rollback() {
    let (_directory, store, owner_id) = fixture().await;
    let transaction = store
        .connection()
        .transaction()
        .await
        .expect("main transaction");
    let claimed = store
        .claim_skill_namespace(owner_id, "owner", OffsetDateTime::now_utc())
        .await
        .expect("namespace claim on independent connection");
    transaction.rollback().await.expect("main rollback");

    assert_eq!(
        store
            .get_skill_namespace(owner_id)
            .await
            .expect("namespace"),
        Some(claimed),
        "an unrelated transaction must not roll back an acknowledged claim"
    );
}

#[tokio::test]
async fn skill_creation_survives_main_connection_rollback() {
    let (_directory, store, owner_id) = fixture().await;
    let now = OffsetDateTime::now_utc();
    let metadata = metadata("original");
    store
        .claim_skill_namespace(owner_id, "owner", now)
        .await
        .expect("namespace");
    // A deferred transaction without statements holds no read or write lock.
    let transaction = store
        .connection()
        .transaction()
        .await
        .expect("main transaction");
    let created = store
        .create_skill(owner_id, "review", &metadata, now)
        .await
        .expect("skill creation on independent connection");
    transaction.rollback().await.expect("main rollback");

    assert_eq!(created.uploaded_version, 1);
    assert_eq!(
        store
            .get_skill_version(created.detail.skill.id, 1)
            .await
            .expect("version"),
        Some(SkillVersionRecord {
            skill_id: created.detail.skill.id,
            version: 1,
            metadata,
            created_at: created.detail.versions[0].created_at,
        }),
        "an unrelated transaction must not roll back a committed skill"
    );
    assert_eq!(
        store
            .get_skill(created.detail.skill.id)
            .await
            .expect("skill"),
        Some(created.detail.skill)
    );
}

#[tokio::test]
async fn skill_append_survives_main_connection_rollback() {
    let (_directory, store, owner_id) = fixture().await;
    let skill_id = create_review_skill(&store, owner_id).await;
    let transaction = store
        .connection()
        .transaction()
        .await
        .expect("main transaction");
    let metadata = metadata("updated");
    let appended = store
        .append_skill_version(owner_id, skill_id, &metadata, OffsetDateTime::now_utc())
        .await
        .expect("append on independent connection");
    transaction.rollback().await.expect("main rollback");

    assert_eq!(appended.uploaded_version, 2);
    assert_eq!(
        store.get_skill_version(skill_id, 2).await.expect("version"),
        Some(SkillVersionRecord {
            skill_id,
            version: 2,
            metadata,
            created_at: appended.detail.versions[0].created_at,
        })
    );
    let skill = store
        .get_skill(skill_id)
        .await
        .expect("skill")
        .expect("exists");
    assert_eq!((skill.default_version, skill.latest_version), (1, 2));
    assert_eq!(skill, appended.detail.skill);
}

#[tokio::test]
async fn default_change_survives_main_connection_rollback() {
    let (_directory, store, owner_id) = fixture().await;
    let skill_id = create_review_skill(&store, owner_id).await;
    let now = OffsetDateTime::now_utc();
    store
        .append_skill_version(owner_id, skill_id, &metadata("updated"), now)
        .await
        .expect("second version");
    let transaction = store
        .connection()
        .transaction()
        .await
        .expect("main transaction");
    let changed = store
        .set_skill_default_version(owner_id, skill_id, 2, now)
        .await
        .expect("default change on independent connection");
    transaction.rollback().await.expect("main rollback");

    let skill = store
        .get_skill(skill_id)
        .await
        .expect("skill")
        .expect("exists");
    assert_eq!(skill, changed);
    assert_eq!((skill.default_version, skill.latest_version), (2, 2));
    assert_eq!(skill.description, "updated");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn simultaneous_skill_appends_allocate_distinct_versions() {
    let (_directory, store, owner_id) = fixture().await;
    let skill_id = create_review_skill(&store, owner_id).await;
    let barrier = Arc::new(Barrier::new(3));
    let mut tasks = Vec::new();
    for description in ["first append", "second append"] {
        let store = store.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            let metadata = metadata(description);
            barrier.wait().await;
            let appended = store
                .append_skill_version(owner_id, skill_id, &metadata, OffsetDateTime::now_utc())
                .await
                .expect("concurrent append");
            (appended, metadata)
        }));
    }
    barrier.wait().await;
    let mut versions = Vec::new();
    for task in tasks {
        let (appended, metadata) = task.await.expect("append task");
        versions.push(appended.uploaded_version);
        assert_eq!(
            appended.detail.skill.latest_version,
            appended.uploaded_version
        );
        assert_eq!(appended.detail.skill.default_version, 1);
        assert_eq!(appended.detail.skill.description, "original");
        assert_eq!(
            appended
                .detail
                .versions
                .iter()
                .map(|version| version.version)
                .collect::<Vec<_>>(),
            (1..=appended.uploaded_version).rev().collect::<Vec<_>>()
        );
        assert_eq!(
            store
                .get_skill_version(skill_id, appended.uploaded_version)
                .await
                .expect("stored version"),
            Some(SkillVersionRecord {
                skill_id,
                version: appended.uploaded_version,
                metadata,
                created_at: appended.detail.versions[0].created_at,
            })
        );
    }

    versions.sort_unstable();
    assert_eq!(versions, [2, 3]);
    let skill = store
        .get_skill(skill_id)
        .await
        .expect("skill")
        .expect("exists");
    assert_eq!((skill.default_version, skill.latest_version), (1, 3));
    assert_eq!(skill.description, "original");
}

async fn install_invalid_version_timestamp(store: &LibsqlStore) {
    store
        .connection()
        .execute(
            "CREATE TRIGGER invalid_version_timestamp AFTER INSERT ON skill_versions
             BEGIN
                 UPDATE skill_versions SET created_at = 9223372036854775807
                 WHERE skill_id = NEW.skill_id AND version = NEW.version;
             END",
            (),
        )
        .await
        .expect("install invalid response timestamp trigger");
}

#[tokio::test]
async fn skill_creation_rolls_back_when_response_decoding_fails() {
    let (_directory, store, owner_id) = fixture().await;
    let now = OffsetDateTime::now_utc();
    store
        .claim_skill_namespace(owner_id, "owner", now)
        .await
        .expect("namespace");
    install_invalid_version_timestamp(&store).await;

    let result = store
        .create_skill(owner_id, "review", &metadata("original"), now)
        .await;
    assert!(matches!(result, Err(StoreError::Serialization(_))));
    assert_eq!(
        store
            .get_skill_by_name("owner", "review")
            .await
            .expect("skill lookup after rollback"),
        None
    );
    let mut rows = store
        .connection()
        .query("SELECT COUNT(*) FROM skill_versions", ())
        .await
        .expect("version count");
    let row = rows.next().await.expect("next").expect("count");
    assert_eq!(row.get::<i64>(0).expect("version count"), 0);
}

#[tokio::test]
async fn skill_append_rolls_back_when_response_decoding_fails() {
    let (_directory, store, owner_id) = fixture().await;
    let skill_id = create_review_skill(&store, owner_id).await;
    let original = store.get_skill(skill_id).await.expect("original skill");
    install_invalid_version_timestamp(&store).await;

    let result = store
        .append_skill_version(
            owner_id,
            skill_id,
            &metadata("updated"),
            OffsetDateTime::now_utc(),
        )
        .await;
    assert!(matches!(result, Err(StoreError::Serialization(_))));
    assert_eq!(
        store.get_skill(skill_id).await.expect("rolled back skill"),
        original
    );
    assert_eq!(
        store
            .get_skill_version(skill_id, 2)
            .await
            .expect("version lookup after rollback"),
        None
    );
}
