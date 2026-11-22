use std::time::Duration;

use tokio::sync::Semaphore;

#[derive(Debug)]
pub(crate) enum BlockingCredentialError {
    Busy,
    Unavailable,
    Failed,
}

/// Keeps the admission permit in the blocking task, even if its caller is cancelled.
pub(crate) async fn run_limited<T, F>(
    slots: &'static Semaphore,
    wait: Duration,
    work: F,
) -> Result<T, BlockingCredentialError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let slot = tokio::time::timeout(wait, slots.acquire())
        .await
        .map_err(|_| BlockingCredentialError::Busy)?
        .map_err(|_| BlockingCredentialError::Unavailable)?;
    tokio::task::spawn_blocking(move || {
        let _slot = slot;
        work()
    })
    .await
    .map_err(|_| BlockingCredentialError::Failed)
}

pub(crate) fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}
