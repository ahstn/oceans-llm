use std::sync::Arc;

use gateway_core::{
    ApiKeyOwnerKind, BudgetAlertChannel, BudgetAlertDeliveryRecord, BudgetAlertDeliveryStatus,
    BudgetAlertRecord, BudgetAlertRepository, BudgetCadence, ExternalMcpAuthMode,
    ExternalMcpTransport, McpRegistryRepository, Money4, NewExternalMcpServerRecord,
    UpdateExternalMcpServerRecord,
};
use serde_json::Map;
use tempfile::{TempDir, tempdir};
use time::OffsetDateTime;
use uuid::Uuid;

use super::LibsqlStore;
use crate::run_migrations;

async fn fixture() -> (TempDir, LibsqlStore) {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("transactions.db");
    run_migrations(&path).await.expect("migrations");
    let store = LibsqlStore::new_local(path.to_str().expect("database path"))
        .await
        .expect("store");
    (directory, store)
}

// Another request can hold a transaction on the shared connection at any time. A deferred
// transaction without statements holds no lock, so it isolates the nested-BEGIN failure.

#[tokio::test]
async fn store_transaction_starts_while_main_connection_is_in_a_transaction() {
    let (_directory, store) = fixture().await;
    let transaction = store
        .connection()
        .transaction()
        .await
        .expect("main transaction");

    let claimed = store
        .claim_pending_budget_alert_delivery_tasks(10, OffsetDateTime::now_utc())
        .await
        .expect("claim runs its own transaction");

    transaction.rollback().await.expect("main rollback");
    assert!(claimed.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn store_write_waits_for_another_store_transaction() {
    let (_directory, store) = fixture().await;
    let now = OffsetDateTime::now_utc();
    let server = store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            ownership: Default::default(),
            server_key: "docs".into(),
            display_name: "Docs".into(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://docs.example.com/mcp".into(),
            auth_mode: ExternalMcpAuthMode::None,
            auth_config: Map::new(),
            timeout_ms: 30_000,
            created_at: now,
        })
        .await
        .expect("server");
    let store = Arc::new(store);
    // Holds the write lock, as a concurrent request's transaction would.
    let holder = store
        .begin_transaction()
        .await
        .expect("holding transaction");

    let update = tokio::spawn({
        let store = store.clone();
        let server = server.clone();
        async move {
            store
                .update_external_mcp_server(&UpdateExternalMcpServerRecord {
                    mcp_server_id: server.mcp_server_id,
                    display_name: "Renamed docs".into(),
                    description: None,
                    server_url: server.server_url,
                    auth_mode: ExternalMcpAuthMode::None,
                    auth_config: Map::new(),
                    timeout_ms: 30_000,
                    owner_contact: None,
                    visibility: Default::default(),
                    expected_owner_team_id: None,
                    expected_visibility: Default::default(),
                    retarget_owner_credentials_only: false,
                    updated_at: now,
                })
                .await
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(!update.is_finished(), "the update waits for the write lock");
    holder.commit().await.expect("holder commit");

    let updated = update
        .await
        .expect("update task")
        .expect("update commits once the lock is free");
    assert_eq!(updated.current.display_name, "Renamed docs");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_store_transactions_do_not_nest() {
    let (_directory, store) = fixture().await;
    let now = OffsetDateTime::now_utc();
    let alert = BudgetAlertRecord {
        budget_alert_id: Uuid::new_v4(),
        ownership_scope_key: format!("user:{}", Uuid::new_v4()),
        owner_kind: ApiKeyOwnerKind::User,
        owner_id: Uuid::new_v4(),
        owner_name: "Member".into(),
        budget_id: Uuid::new_v4(),
        cadence: BudgetCadence::Monthly,
        threshold_bps: 2_000,
        window_start: now - time::Duration::days(1),
        window_end: now + time::Duration::days(29),
        spend_before_usd: Money4::from_scaled(7_500_000),
        spend_after_usd: Money4::from_scaled(8_200_000),
        remaining_budget_usd: Money4::from_scaled(1_800_000),
        created_at: now,
        updated_at: now,
    };
    let delivery = BudgetAlertDeliveryRecord {
        budget_alert_delivery_id: Uuid::new_v4(),
        budget_alert_id: alert.budget_alert_id,
        channel: BudgetAlertChannel::Email,
        delivery_status: BudgetAlertDeliveryStatus::Pending,
        recipient: Some("member@example.com".into()),
        provider_message_id: None,
        failure_reason: None,
        queued_at: now,
        last_attempted_at: None,
        sent_at: None,
        updated_at: now,
    };
    assert!(
        store
            .create_budget_alert_with_deliveries(&alert, std::slice::from_ref(&delivery))
            .await
            .expect("pending delivery")
    );
    let store = Arc::new(store);
    let claims = (0..32).map(|_| {
        let store = store.clone();
        tokio::spawn(async move {
            store
                .claim_pending_budget_alert_delivery_tasks(10, OffsetDateTime::now_utc())
                .await
        })
    });

    let mut claimed = Vec::new();
    for claim in futures_util::future::join_all(claims).await {
        claimed.extend(
            claim
                .expect("claim task")
                .expect("concurrent claims each run their own transaction"),
        );
    }

    assert_eq!(claimed.len(), 1, "exactly one claim gets the delivery");
    assert_eq!(
        claimed[0].delivery.budget_alert_delivery_id,
        delivery.budget_alert_delivery_id
    );
    assert!(
        store
            .claim_pending_budget_alert_delivery_tasks(10, OffsetDateTime::now_utc())
            .await
            .expect("later claim")
            .is_empty(),
        "a claimed delivery is no longer claimable"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn waiting_for_the_write_lock_does_not_block_the_runtime() {
    let (_directory, store) = fixture().await;
    let store = Arc::new(store);
    let holder = store
        .begin_transaction()
        .await
        .expect("holding transaction");
    let waiter = tokio::spawn({
        let store = store.clone();
        async move { store.begin_transaction().await }
    });

    // On one thread, a blocking wait would keep this task from committing until the waiter
    // gave up with "database is locked".
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    holder.commit().await.expect("holder commit");

    let transaction = waiter
        .await
        .expect("waiter task")
        .expect("the waiter gets the write lock once it is free");
    transaction.rollback().await.expect("waiter rollback");
}

#[tokio::test(flavor = "current_thread")]
async fn shared_cache_memory_store_serializes_transactions() {
    let store = Arc::new(
        LibsqlStore::new_local(":memory:")
            .await
            .expect("memory store"),
    );
    let holder = store
        .begin_transaction()
        .await
        .expect("holding transaction");
    let waiter = tokio::spawn({
        let store = store.clone();
        async move { store.begin_transaction().await }
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    holder.commit().await.expect("holder commit");

    let transaction = waiter
        .await
        .expect("waiter task")
        .expect("a shared-cache lock is retried until it is free");
    transaction.rollback().await.expect("waiter rollback");
}

#[tokio::test]
async fn shared_cache_reader_does_not_block_a_transaction_write() {
    let store = LibsqlStore::new_local(":memory:")
        .await
        .expect("memory store");
    store
        .connection()
        .execute_batch(
            "CREATE TABLE probe (value INTEGER); INSERT INTO probe VALUES (1), (2), (3);",
        )
        .await
        .expect("probe table");
    // An unfinished cursor on the shared connection, as a concurrent request's read would be.
    let mut reader = store
        .connection()
        .query("SELECT value FROM probe", ())
        .await
        .expect("reader");
    reader.next().await.expect("first row").expect("row");

    let transaction = store.begin_transaction().await.expect("transaction");
    transaction
        .execute("INSERT INTO probe VALUES (4)", ())
        .await
        .expect("a shared-cache reader does not lock the table against this write");
    transaction.commit().await.expect("commit");
    drop(reader);
}
