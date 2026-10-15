use std::sync::Arc;

use gateway_core::{
    BudgetAlertRepository, ExternalMcpAuthMode, ExternalMcpTransport, McpRegistryRepository,
    NewExternalMcpServerRecord, UpdateExternalMcpServerRecord,
};
use serde_json::Map;
use tempfile::{TempDir, tempdir};
use time::OffsetDateTime;

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
    assert_eq!(updated.display_name, "Renamed docs");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_store_transactions_do_not_nest() {
    let (_directory, store) = fixture().await;
    let store = Arc::new(store);
    let claims = (0..32).map(|_| {
        let store = store.clone();
        tokio::spawn(async move {
            store
                .claim_pending_budget_alert_delivery_tasks(10, OffsetDateTime::now_utc())
                .await
        })
    });

    for claim in futures_util::future::join_all(claims).await {
        claim
            .expect("claim task")
            .expect("concurrent claims each run their own transaction");
    }
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
