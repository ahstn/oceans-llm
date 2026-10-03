use gateway_core::skills::{SkillListQuery, SkillManifest, SkillRepository, SkillVersionMetadata};
use gateway_core::{AuthMode, GlobalRole, StoreError, UserStatus};
use serial_test::serial;
use tempfile::tempdir;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::tests::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    AnyStore, GatewayStore, LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations,
    run_migrations_with_options,
};

#[tokio::test]
async fn libsql_skill_namespace_ownership_and_versions() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("skills.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = AnyStore::Libsql(
        LibsqlStore::new_local(db_path.to_str().expect("path"))
            .await
            .expect("store"),
    );
    exercise_skills(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_skill_namespace_ownership_and_versions() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres skills test: TEST_POSTGRES_URL is not set");
        return;
    };
    run_migrations_with_options(&StoreConnectionOptions::Postgres {
        url: test_db.database_url.clone(),
        max_connections: 4,
    })
    .await
    .expect("migrations");
    let store = AnyStore::Postgres(
        PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("store"),
    );
    let (owner_id, skill_id) = exercise_skills(&store).await;
    let now = OffsetDateTime::now_utc();
    let one = metadata("concurrent-one");
    let two = metadata("concurrent-two");
    let (first, second) = tokio::join!(
        store.append_skill_version(owner_id, skill_id, &one, now),
        store.append_skill_version(owner_id, skill_id, &two, now),
    );
    let mut versions = [
        first.expect("first concurrent append").version,
        second.expect("second concurrent append").version,
    ];
    versions.sort_unstable();
    assert_eq!(versions, [3, 4]);
    let skill = store
        .get_skill(skill_id)
        .await
        .expect("skill")
        .expect("exists");
    assert_eq!((skill.default_version, skill.latest_version), (2, 4));
    drop(store);
    drop_postgres_test_database(&test_db).await;
}

fn metadata(label: &str) -> SkillVersionMetadata {
    SkillVersionMetadata {
        description: label.to_string(),
        sha256: "a".repeat(64),
        object_key: format!("skills/{label}/{}.zip", Uuid::new_v4()),
        archive_bytes: 100,
        extracted_bytes: 200,
        file_count: 1,
        manifest: SkillManifest {
            name: "review".to_string(),
            description: label.to_string(),
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
        instructions: format!("---\nname: review\ndescription: {label}\n---\nReview changes."),
    }
}

async fn exercise_skills(store: &AnyStore) -> (Uuid, Uuid) {
    let first = store
        .create_identity_user(
            "First",
            "first@example.com",
            "first@example.com",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("first user");
    let second = store
        .create_identity_user(
            "Second",
            "second@example.com",
            "second@example.com",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("second user");
    let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("timestamp");
    let claimed = store
        .claim_skill_namespace(first.user_id, "first", now)
        .await
        .expect("claim");
    assert_eq!(
        store
            .claim_skill_namespace(first.user_id, "first", now)
            .await
            .expect("idempotent"),
        claimed
    );
    assert!(matches!(
        store
            .claim_skill_namespace(first.user_id, "renamed", now)
            .await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store
            .claim_skill_namespace(second.user_id, "first", now)
            .await,
        Err(StoreError::Conflict(_))
    ));
    assert!(
        store
            .get_skill_namespace(second.user_id)
            .await
            .expect("namespace")
            .is_none()
    );
    store
        .claim_skill_namespace(second.user_id, "second", now)
        .await
        .expect("second namespace");

    let original = metadata("original");
    let first_version = store
        .create_skill(first.user_id, "review", &original, now)
        .await
        .expect("create skill");
    let other_version = store
        .create_skill(second.user_id, "review", &metadata("other"), now)
        .await
        .expect("same name different owner");
    assert_ne!(first_version.skill_id, other_version.skill_id);
    assert!(matches!(
        store
            .create_skill(first.user_id, "review", &metadata("duplicate"), now)
            .await,
        Err(StoreError::Conflict(_))
    ));
    let listed = store
        .list_skills(&SkillListQuery::default())
        .await
        .expect("list");
    assert_eq!(listed.len(), 2);
    let filtered = store
        .list_skills(&SkillListQuery {
            namespace: Some("first".to_string()),
            ..Default::default()
        })
        .await
        .expect("filter");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, first_version.skill_id);
    assert_eq!(
        store
            .get_skill_by_name("second", "review")
            .await
            .expect("lookup")
            .expect("exists")
            .id,
        other_version.skill_id
    );
    assert!(
        store
            .get_skill_by_name("first", "missing")
            .await
            .expect("lookup")
            .is_none()
    );

    exercise_versions(
        store,
        first.user_id,
        second.user_id,
        first_version.skill_id,
        &original,
        now,
    )
    .await;
    (first.user_id, first_version.skill_id)
}

async fn exercise_versions(
    store: &AnyStore,
    owner_id: Uuid,
    other_id: Uuid,
    skill_id: Uuid,
    original: &SkillVersionMetadata,
    now: OffsetDateTime,
) {
    let next = metadata("updated");
    assert!(matches!(
        store
            .append_skill_version(other_id, skill_id, &next, now)
            .await,
        Err(StoreError::NotFound(_))
    ));
    let appended = store
        .append_skill_version(owner_id, skill_id, &next, now)
        .await
        .expect("append");
    assert_eq!(appended.version, 2);
    let skill = store
        .get_skill(skill_id)
        .await
        .expect("skill")
        .expect("exists");
    assert_eq!((skill.default_version, skill.latest_version), (1, 2));
    assert_eq!(skill.description, "original");
    assert!(matches!(
        store
            .append_skill_version(owner_id, skill_id, original, now)
            .await,
        Err(StoreError::Conflict(_))
    ));
    assert_eq!(
        store
            .get_skill(skill_id)
            .await
            .expect("skill")
            .expect("exists")
            .latest_version,
        2,
        "failed insert must roll back version allocation"
    );
    assert!(matches!(
        store
            .set_skill_default_version(other_id, skill_id, 2, now)
            .await,
        Err(StoreError::NotFound(_))
    ));
    assert!(matches!(
        store
            .set_skill_default_version(owner_id, skill_id, 99, now)
            .await,
        Err(StoreError::NotFound(_))
    ));
    let skill = store
        .set_skill_default_version(owner_id, skill_id, 2, now)
        .await
        .expect("set default");
    assert_eq!((skill.default_version, skill.latest_version), (2, 2));
    assert_eq!(skill.description, "updated");
    let versions = store.list_skill_versions(skill_id).await.expect("versions");
    assert_eq!(
        versions
            .iter()
            .map(|version| version.version)
            .collect::<Vec<_>>(),
        [2, 1]
    );
    assert_eq!(
        store
            .get_skill_version(skill_id, 1)
            .await
            .expect("original version")
            .expect("exists")
            .metadata,
        *original,
        "old version content must remain immutable"
    );
    assert_eq!(
        store
            .get_skill_version(skill_id, 2)
            .await
            .expect("version")
            .expect("exists"),
        appended
    );
    assert!(
        store
            .get_skill_version(skill_id, 3)
            .await
            .expect("missing version")
            .is_none()
    );
}
