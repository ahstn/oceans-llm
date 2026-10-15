//! Conditional writes report whether they changed anything, and single-record
//! updates return the row they replaced. The admin audit records a change from
//! these results rather than from a comparison against an earlier read.

use crate::tests::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    GatewayStore, LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations,
    run_migrations_with_options,
};
use gateway_core::{
    AuthMode, GlobalRole, McpAccessRepository, MembershipRole, NewMcpToolsetRecord, RequestTag,
    UpdateMcpToolsetRecord, UserStatus,
};
use serial_test::serial;
use tempfile::tempdir;
use time::{Duration, OffsetDateTime};

#[tokio::test]
#[serial]
async fn libsql_updates_report_whether_they_changed_anything() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_identity_change_reports(&store).await;
    exercise_toolset_change_reports(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_updates_report_whether_they_changed_anything() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres change report test: TEST_POSTGRES_URL is not set");
        return;
    };
    run_migrations_with_options(&StoreConnectionOptions::Postgres {
        url: test_db.database_url.clone(),
        max_connections: 4,
    })
    .await
    .expect("migrations");
    let store = PostgresStore::connect(&test_db.database_url, 4)
        .await
        .expect("store");
    exercise_identity_change_reports(&store).await;
    exercise_toolset_change_reports(&store).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}

fn tag(key: &str, value: &str) -> RequestTag {
    RequestTag {
        key: key.to_string(),
        value: value.to_string(),
    }
}

async fn exercise_identity_change_reports<S: GatewayStore + ?Sized>(store: &S) {
    let now = OffsetDateTime::now_utc();
    let team = store
        .create_team("platform", "Platform")
        .await
        .expect("team");
    assert!(
        !store
            .update_team_name(team.team_id, "Platform", now)
            .await
            .expect("same name")
    );
    assert!(
        store
            .update_team_name(team.team_id, "Platform Ops", now)
            .await
            .expect("rename")
    );
    let tags = [tag("env", "prod")];
    assert!(
        store
            .update_team_tags(team.team_id, &tags, now)
            .await
            .expect("tag")
    );
    assert!(
        !store
            .update_team_tags(team.team_id, &tags, now)
            .await
            .expect("same tags")
    );

    let user = store
        .create_identity_user(
            "Member",
            "member@example.test",
            "member@example.test",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("user");
    assert!(
        !store
            .update_identity_user(user.user_id, GlobalRole::User, AuthMode::Password, now)
            .await
            .expect("same role")
    );
    assert!(
        store
            .update_identity_user(
                user.user_id,
                GlobalRole::PlatformAdmin,
                AuthMode::Password,
                now
            )
            .await
            .expect("promote")
    );
    assert!(
        store
            .update_user_tags(user.user_id, &tags, now)
            .await
            .expect("tag user")
    );
    assert!(
        !store
            .update_user_tags(user.user_id, &tags, now)
            .await
            .expect("same user tags")
    );
    assert!(
        !store
            .delete_user_password_auth(user.user_id)
            .await
            .expect("no password to delete")
    );
    assert!(
        !store
            .clear_user_oidc_link(user.user_id)
            .await
            .expect("no link to clear")
    );

    store
        .assign_team_membership(user.user_id, team.team_id, MembershipRole::Member)
        .await
        .expect("assign");
    assert!(
        !store
            .update_team_membership_role(team.team_id, user.user_id, MembershipRole::Member, now)
            .await
            .expect("same role")
    );
    assert!(
        store
            .update_team_membership_role(team.team_id, user.user_id, MembershipRole::Admin, now)
            .await
            .expect("promote member")
    );

    let account = store
        .create_service_account(team.team_id, "ci", "CI", now)
        .await
        .expect("service account");
    assert!(
        store
            .update_service_account_profile(account.service_account_id, "CI", &[], now)
            .await
            .expect("same profile")
            .is_none()
    );
    let replaced = store
        .update_service_account_profile(
            account.service_account_id,
            "CI runner",
            &tags,
            now + Duration::seconds(1),
        )
        .await
        .expect("rename account")
        .expect("the rename replaced the profile");
    assert_eq!(replaced.service_account_name, "CI");
    assert!(replaced.tags.is_empty());

    // Disabling returns the account the update disabled, as it was then.
    let disabled = store
        .disable_service_account(account.service_account_id, now + Duration::seconds(2))
        .await
        .expect("disable")
        .expect("the account was active");
    assert_eq!(disabled.service_account_name, "CI runner");
    assert!(
        store
            .disable_service_account(account.service_account_id, now + Duration::seconds(3))
            .await
            .expect("disable again")
            .is_none()
    );
}

async fn exercise_toolset_change_reports<S: GatewayStore + McpAccessRepository + ?Sized>(
    store: &S,
) {
    let now = OffsetDateTime::now_utc();
    let toolset = store
        .create_mcp_toolset(&NewMcpToolsetRecord {
            toolset_key: "triage".to_string(),
            display_name: "Triage".to_string(),
            description: None,
            created_at: now,
        })
        .await
        .expect("toolset");
    let details = |display_name: &str| UpdateMcpToolsetRecord {
        toolset_id: toolset.toolset_id,
        display_name: display_name.to_string(),
        description: None,
        updated_at: now + Duration::seconds(1),
    };
    let unchanged = store
        .update_mcp_toolset(&details("Triage"))
        .await
        .expect("same details");
    assert!(!unchanged.changed());
    assert_eq!(unchanged.current.updated_at, toolset.updated_at);
    let renamed = store
        .update_mcp_toolset(&details("Incident triage"))
        .await
        .expect("rename");
    assert_eq!(renamed.current.display_name, "Incident triage");
    assert_eq!(
        renamed.previous.map(|previous| previous.display_name),
        Some("Triage".to_string())
    );

    let tools = store
        .replace_mcp_toolset_tools(toolset.toolset_id, &[], now)
        .await
        .expect("empty set");
    assert!(!tools.changed(), "an empty set replaced by an empty set");
}
