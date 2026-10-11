use gateway_core::skills::{
    SkillListQuery, SkillManifest, SkillRepository, SkillUploadResponse, SkillVersionMetadata,
};
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
    let mut one = metadata("concurrent-one");
    one.sha256 = "b".repeat(64);
    let mut two = metadata("concurrent-two");
    two.sha256 = "c".repeat(64);
    let (first, second) = tokio::join!(
        store.append_skill_version(owner_id, skill_id, &one, now),
        store.append_skill_version(owner_id, skill_id, &two, now),
    );
    let first = first.expect("first concurrent append");
    let second = second.expect("second concurrent append");
    assert_upload_response(&first, &one, 2, "updated");
    assert_upload_response(&second, &two, 2, "updated");
    let mut versions = [first.uploaded_version, second.uploaded_version];
    versions.sort_unstable();
    assert_eq!(versions, [3, 4]);
    let skill = store
        .get_skill(skill_id)
        .await
        .expect("skill")
        .expect("exists");
    assert_eq!((skill.default_version, skill.latest_version), (2, 4));
    let AnyStore::Postgres(postgres) = &store else {
        unreachable!("postgres fixture")
    };
    exercise_postgres_response_rollback(postgres, owner_id, skill_id).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}

async fn exercise_postgres_response_rollback(
    store: &PostgresStore,
    owner_id: Uuid,
    skill_id: Uuid,
) {
    let before = store
        .get_skill(skill_id)
        .await
        .expect("skill before append");
    sqlx::query("UPDATE skill_versions SET created_at = $1 WHERE skill_id = $2 AND version = 1")
        .bind(i64::MAX)
        .bind(skill_id.to_string())
        .execute(store.pool())
        .await
        .expect("inject invalid older version summary");
    let result = store
        .append_skill_version(
            owner_id,
            skill_id,
            &metadata("response-fails"),
            OffsetDateTime::now_utc(),
        )
        .await;
    assert!(matches!(result, Err(StoreError::Serialization(_))));
    assert_eq!(
        store.get_skill(skill_id).await.expect("rolled back skill"),
        before
    );
    assert!(
        store
            .get_skill_version(skill_id, 5)
            .await
            .expect("rolled back version")
            .is_none(),
        "response decoding must complete before committing the new version"
    );
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

fn assert_upload_response(
    response: &SkillUploadResponse,
    uploaded: &SkillVersionMetadata,
    default_version: u32,
    description: &str,
) {
    let detail = &response.detail;
    assert_eq!(detail.skill.latest_version, response.uploaded_version);
    assert_eq!(detail.skill.default_version, default_version);
    assert_eq!(detail.skill.description, description);
    assert_eq!(
        detail
            .versions
            .iter()
            .map(|version| version.version)
            .collect::<Vec<_>>(),
        (1..=response.uploaded_version).rev().collect::<Vec<_>>(),
        "the response must reflect the same transaction as its uploaded version"
    );
    let latest = &detail.versions[0];
    assert_eq!(latest.sha256, uploaded.sha256);
    assert_eq!(latest.archive_bytes, uploaded.archive_bytes);
    assert_eq!(latest.extracted_bytes, uploaded.extracted_bytes);
    assert_eq!(latest.file_count, uploaded.file_count);
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
    assert_eq!(first_version.uploaded_version, 1);
    assert_upload_response(&first_version, &original, 1, "original");
    assert_eq!(first_version.detail.skill.owner_user_id, first.user_id);
    assert_eq!(first_version.detail.skill.namespace, "first");
    assert_eq!(first_version.detail.skill.name, "review");
    let other_version = store
        .create_skill(second.user_id, "review", &metadata("other"), now)
        .await
        .expect("same name different owner");
    assert_ne!(first_version.detail.skill.id, other_version.detail.skill.id);
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
    assert_eq!(filtered[0].id, first_version.detail.skill.id);
    assert_eq!(
        store
            .get_skill_by_name("second", "review")
            .await
            .expect("lookup")
            .expect("exists")
            .id,
        other_version.detail.skill.id
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
        first_version.detail.skill.id,
        &original,
        now,
    )
    .await;
    exercise_skill_search(store, first.user_id, second.user_id, now).await;
    (first.user_id, first_version.detail.skill.id)
}

async fn exercise_skill_search(
    store: &AnyStore,
    first_owner: Uuid,
    second_owner: Uuid,
    now: OffsetDateTime,
) {
    let name_match = store
        .create_skill(first_owner, "name-needle", &metadata("Utility"), now)
        .await
        .expect("skill matching by name")
        .detail
        .skill
        .id;
    let description_match = store
        .create_skill(
            second_owner,
            "catalog",
            &metadata("Needle in Description: literal 100% and under_score; Café ÉLAN"),
            now + time::Duration::seconds(1),
        )
        .await
        .expect("skill matching by description")
        .detail
        .skill
        .id;
    store
        .create_skill(
            first_owner,
            "other",
            &metadata("Plain utility"),
            now + time::Duration::seconds(2),
        )
        .await
        .expect("newer skill outside search results");

    for (search, expected) in [
        ("NAME-N", vec![name_match]),
        ("description", vec![description_match]),
        ("  NeEdLe\t", vec![description_match, name_match]),
        ("%", vec![description_match]),
        ("_", vec![description_match]),
        ("CAFé", vec![description_match]),
        ("CAFÉ", vec![]),
        ("Élan", vec![description_match]),
        ("élan", vec![]),
        ("absent", vec![]),
    ] {
        let results = store
            .list_skills(&SkillListQuery {
                q: Some(search.to_string()),
                ..Default::default()
            })
            .await
            .expect("search skills");
        assert_eq!(
            results.iter().map(|skill| skill.id).collect::<Vec<_>>(),
            expected,
            "query {search:?}"
        );
    }

    let by_owner = store
        .list_skills(&SkillListQuery {
            q: Some("SECond".to_string()),
            ..Default::default()
        })
        .await
        .expect("search owner namespace");
    assert_eq!(by_owner.len(), 2);
    assert!(by_owner.iter().all(|skill| skill.namespace == "second"));

    let scoped = store
        .list_skills(&SkillListQuery {
            namespace: Some("first".to_string()),
            q: Some("needle".to_string()),
            ..Default::default()
        })
        .await
        .expect("combine namespace and search");
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].id, name_match);

    for (offset, expected) in [
        (0, vec![description_match]),
        (1, vec![name_match]),
        (2, vec![]),
    ] {
        let page = store
            .list_skills(&SkillListQuery {
                q: Some("needle".to_string()),
                limit: 1,
                offset,
                ..Default::default()
            })
            .await
            .expect("search before pagination");
        assert_eq!(
            page.iter().map(|skill| skill.id).collect::<Vec<_>>(),
            expected,
            "offset {offset}"
        );
    }

    let unfiltered = store
        .list_skills(&SkillListQuery::default())
        .await
        .expect("all skills");
    let blank = store
        .list_skills(&SkillListQuery {
            q: Some(" \t\n ".to_string()),
            ..Default::default()
        })
        .await
        .expect("blank search is ignored");
    assert_eq!(blank, unfiltered);
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
    assert_eq!(appended.uploaded_version, 2);
    assert_upload_response(&appended, &next, 1, "original");
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
            .expect("exists")
            .metadata,
        next
    );
    assert!(
        store
            .get_skill_version(skill_id, 3)
            .await
            .expect("missing version")
            .is_none()
    );
}
