use super::run_report_with_heartbeat;
use gateway_core::GatewayError;
use tokio::sync::{Mutex, oneshot};
use tokio::time::{Duration, timeout};

#[tokio::test]
async fn publication_progresses_while_renewal_waits_for_the_queue_lock() {
    let queue = Mutex::new(Vec::new());
    let (report_locked_tx, report_locked_rx) = oneshot::channel();
    let (renewal_started_tx, renewal_started_rx) = oneshot::channel();
    let (renewal_finished_tx, renewal_finished_rx) = oneshot::channel();
    let (stop_heartbeat_tx, stop_heartbeat_rx) = oneshot::channel::<()>();

    let report = async {
        let mut transaction = queue.lock().await;
        transaction.push("publishing");
        report_locked_tx.send(()).expect("heartbeat is listening");
        renewal_started_rx.await.expect("renewal started");
        // Commit must keep progressing even while renewal is waiting on this lock.
        transaction.push("published");
        drop(transaction);
        renewal_finished_rx
            .await
            .expect("renewal acquired the lock");
        Ok::<(), GatewayError>(())
    };
    let heartbeat = async {
        report_locked_rx.await.expect("publication holds the lock");
        renewal_started_tx.send(()).expect("report is listening");
        let mut lease = queue.lock().await;
        lease.push("renewed");
        drop(lease);
        renewal_finished_tx.send(()).expect("report is listening");
        stop_heartbeat_rx
            .await
            .expect("heartbeat was not cancelled");
        Ok(())
    };

    timeout(
        Duration::from_secs(1),
        run_report_with_heartbeat(report, heartbeat),
    )
    .await
    .expect("publication and renewal must not deadlock")
    .expect("publication completed");
    assert_eq!(*queue.lock().await, ["publishing", "published", "renewed"]);
    assert!(stop_heartbeat_tx.send(()).is_err());
}

#[tokio::test]
async fn lost_lease_drops_unfinished_publication_before_queue_cleanup() {
    let queue = Mutex::new(Vec::new());
    let (report_locked_tx, report_locked_rx) = oneshot::channel();
    let (publish_tx, publish_rx) = oneshot::channel();

    let report = async {
        let mut transaction = queue.lock().await;
        report_locked_tx.send(()).expect("heartbeat is listening");
        publish_rx.await.expect("publication was allowed to commit");
        transaction.push("published");
        Ok(())
    };
    let heartbeat = async {
        report_locked_rx.await.expect("publication holds the lock");
        Err(GatewayError::Internal("lease lost".to_string()))
    };

    let result = timeout(
        Duration::from_secs(1),
        run_report_with_heartbeat(report, heartbeat),
    )
    .await
    .expect("lease loss must cancel publication");
    assert!(matches!(result, Err(GatewayError::Internal(message)) if message == "lease lost"));
    assert!(publish_tx.send(()).is_err(), "publication must be dropped");
    let mut cleanup = queue
        .try_lock()
        .expect("publication must release its lock before the failure write");
    assert!(cleanup.is_empty());
    cleanup.push("failed");
    assert_eq!(*cleanup, ["failed"]);
}
