use std::{collections::VecDeque, sync::Mutex, time::Duration};

use async_trait::async_trait;

use crate::{
    EvaluationError, EvaluationInput, ManagedEvaluator, ManagedOutcome, ManagedService, ReasonCode,
};

/// A deterministic managed evaluator for guardrail contract tests in gateway crates.
pub struct StubManagedEvaluator {
    id: String,
    service: ManagedService,
    delay: Duration,
    outcomes: Mutex<VecDeque<Result<ManagedOutcome, EvaluationError>>>,
}

impl StubManagedEvaluator {
    pub fn new(
        id: impl Into<String>,
        service: ManagedService,
        outcomes: impl IntoIterator<Item = Result<ManagedOutcome, EvaluationError>>,
    ) -> Self {
        Self {
            id: id.into(),
            service,
            delay: Duration::ZERO,
            outcomes: Mutex::new(outcomes.into_iter().collect()),
        }
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

#[async_trait]
impl ManagedEvaluator for StubManagedEvaluator {
    fn id(&self) -> &str {
        &self.id
    }

    fn service(&self) -> ManagedService {
        self.service
    }

    async fn evaluate(&self, _input: &EvaluationInput) -> Result<ManagedOutcome, EvaluationError> {
        tokio::time::sleep(self.delay).await;
        self.outcomes
            .lock()
            .expect("stub evaluator lock poisoned")
            .pop_front()
            .unwrap_or_else(|| {
                Ok(ManagedOutcome::Allow {
                    reason_code: ReasonCode::new("stub.allow").unwrap(),
                    metadata: Default::default(),
                })
            })
    }
}
