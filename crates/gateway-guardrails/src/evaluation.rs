use std::{collections::BTreeMap, sync::Arc, time::Instant};

use async_trait::async_trait;
use tokio::time::{Duration, timeout};

use crate::{
    ContentTransformation, DecisionAction, DecisionId, DecisionRecord, EffectivePolicy,
    EvaluationInput, FailureDisposition, GuardrailConfig, GuardrailEvaluation,
    ManagedDecisionMetadata, ManagedService, MatchedRule, PolicyMode, ReasonCode,
};

pub trait DeterministicEvaluator: Send + Sync {
    fn id(&self) -> &str;

    fn evaluate(
        &self,
        input: &EvaluationInput,
        policy: &EffectivePolicy,
    ) -> Result<Option<MatchedRule>, EvaluationError>;
}

#[async_trait]
pub trait ManagedEvaluator: Send + Sync {
    fn id(&self) -> &str;
    fn service(&self) -> ManagedService;

    async fn evaluate(&self, input: &EvaluationInput) -> Result<ManagedOutcome, EvaluationError>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum ManagedOutcome {
    Allow {
        reason_code: ReasonCode,
        metadata: ManagedDecisionMetadata,
    },
    Intervention {
        reason_code: ReasonCode,
        metadata: ManagedDecisionMetadata,
    },
    Transformed {
        transformation: ContentTransformation,
        reason_code: ReasonCode,
        metadata: ManagedDecisionMetadata,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvaluationError {
    #[error("guardrail evaluation timed out")]
    Timeout,
    #[error("guardrail content exceeds the configured limit")]
    ContentTooLarge,
    #[error("guardrail service is unavailable: {0}")]
    Unavailable(String),
    #[error("guardrail service denied access")]
    AccessDenied,
    #[error("guardrail service returned a malformed response: {0}")]
    MalformedResponse(String),
    #[error("generated tool call arguments are not a JSON object")]
    MalformedToolCall,
    #[error("guardrail evaluator is not configured: {0}")]
    MissingEvaluator(String),
    #[error("guardrail transformation is invalid for this phase")]
    InvalidTransformation,
}

impl EvaluationError {
    fn reason_code(&self) -> ReasonCode {
        let code = match self {
            Self::Timeout => "managed.timeout",
            Self::ContentTooLarge => "managed.content_too_large",
            Self::Unavailable(_) => "managed.unavailable",
            Self::AccessDenied => "managed.access_denied",
            Self::MalformedResponse(_) => "managed.malformed_response",
            Self::MalformedToolCall => "tool_call.malformed_arguments",
            Self::MissingEvaluator(_) => "managed.missing_evaluator",
            Self::InvalidTransformation => "managed.invalid_transformation",
        };
        ReasonCode::new(code).expect("static reason code is valid")
    }
}

pub struct GuardrailEngine {
    deterministic: Vec<Arc<dyn DeterministicEvaluator>>,
    managed: BTreeMap<String, Arc<dyn ManagedEvaluator>>,
}

impl GuardrailEngine {
    pub fn new(
        deterministic: Vec<Arc<dyn DeterministicEvaluator>>,
        managed: BTreeMap<String, Arc<dyn ManagedEvaluator>>,
    ) -> Self {
        Self {
            deterministic,
            managed,
        }
    }

    pub fn deterministic_only(evaluator: Arc<dyn DeterministicEvaluator>) -> Self {
        Self::new(vec![evaluator], BTreeMap::new())
    }

    pub async fn evaluate(
        &self,
        policy: &EffectivePolicy,
        config: &GuardrailConfig,
        mut input: EvaluationInput,
    ) -> GuardrailEvaluation {
        if !policy.enabled {
            return GuardrailEvaluation {
                action: DecisionAction::Allow,
                decisions: Vec::new(),
                output: input.payload,
            };
        }

        let mut decisions = Vec::new();
        let mut final_action = DecisionAction::Allow;

        for evaluator in &self.deterministic {
            let content_hash = input.payload.content_hash();
            let started = Instant::now();
            match evaluator.evaluate(&input, policy) {
                Ok(Some(matched_rule)) => {
                    let action = action_for_mode(policy.mode);
                    decisions.push(DecisionRecord {
                        action,
                        reason_code: matched_rule.reason_code.clone(),
                        matched_rule: Some(matched_rule),
                        ..base_decision(
                            &input,
                            policy,
                            evaluator.id(),
                            None,
                            DecisionRecord::latency(started.elapsed()),
                            content_hash,
                        )
                    });
                    if action == DecisionAction::Deny {
                        return GuardrailEvaluation {
                            action,
                            decisions,
                            output: input.payload,
                        };
                    }
                    final_action = DecisionAction::Audit;
                }
                Ok(None) => decisions.push(base_decision(
                    &input,
                    policy,
                    evaluator.id(),
                    None,
                    DecisionRecord::latency(started.elapsed()),
                    content_hash,
                )),
                Err(error) => {
                    decisions.push(DecisionRecord {
                        action: DecisionAction::Deny,
                        reason_code: error.reason_code(),
                        failure_disposition: Some(FailureDisposition::FailClosed),
                        ..base_decision(
                            &input,
                            policy,
                            evaluator.id(),
                            None,
                            DecisionRecord::latency(started.elapsed()),
                            content_hash,
                        )
                    });
                    return GuardrailEvaluation {
                        action: DecisionAction::Deny,
                        decisions,
                        output: input.payload,
                    };
                }
            }
        }

        for check_name in &policy.managed_checks {
            let Some(check) = config.managed_checks.get(check_name) else {
                let error = EvaluationError::MissingEvaluator(check_name.clone());
                let decision = failure_decision(
                    check_name,
                    None,
                    &input,
                    policy,
                    FailureDisposition::FailClosed,
                    error,
                    0,
                );
                decisions.push(decision);
                return GuardrailEvaluation {
                    action: DecisionAction::Deny,
                    decisions,
                    output: input.payload,
                };
            };
            if !check.phases.contains(&input.phase) {
                continue;
            }
            let Some(evaluator) = self.managed.get(check_name) else {
                let error = EvaluationError::MissingEvaluator(check_name.clone());
                let decision = failure_decision(
                    check_name,
                    None,
                    &input,
                    policy,
                    check.failure_disposition,
                    error,
                    0,
                );
                let denied = check.failure_disposition == FailureDisposition::FailClosed;
                decisions.push(decision);
                if denied {
                    return GuardrailEvaluation {
                        action: DecisionAction::Deny,
                        decisions,
                        output: input.payload,
                    };
                }
                final_action = DecisionAction::Audit;
                continue;
            };

            let started = Instant::now();
            let result = if input.serialized_byte_len() > check.max_content_bytes {
                Err(EvaluationError::ContentTooLarge)
            } else {
                timeout(
                    Duration::from_millis(check.timeout_ms),
                    evaluator.evaluate(&input),
                )
                .await
                .unwrap_or(Err(EvaluationError::Timeout))
            };
            let latency_micros = DecisionRecord::latency(started.elapsed());
            let content_hash = input.payload.content_hash();

            match result {
                Ok(ManagedOutcome::Allow {
                    reason_code,
                    metadata,
                }) => {
                    decisions.push(DecisionRecord {
                        managed_metadata: Some(metadata),
                        reason_code,
                        ..base_decision(
                            &input,
                            policy,
                            evaluator.id(),
                            Some(evaluator.service()),
                            latency_micros,
                            content_hash,
                        )
                    });
                }
                Ok(ManagedOutcome::Intervention {
                    reason_code,
                    metadata,
                }) => {
                    let action = action_for_mode(policy.mode);
                    decisions.push(DecisionRecord {
                        managed_metadata: Some(metadata),
                        action,
                        reason_code,
                        ..base_decision(
                            &input,
                            policy,
                            evaluator.id(),
                            Some(evaluator.service()),
                            latency_micros,
                            content_hash,
                        )
                    });
                    if action == DecisionAction::Deny {
                        return GuardrailEvaluation {
                            action,
                            decisions,
                            output: input.payload,
                        };
                    }
                    final_action = DecisionAction::Audit;
                }
                Ok(ManagedOutcome::Transformed {
                    transformation,
                    reason_code,
                    metadata,
                }) => {
                    if !input.payload.replace_text(transformation.content) {
                        let decision = failure_decision(
                            evaluator.id(),
                            Some(evaluator.service()),
                            &input,
                            policy,
                            check.failure_disposition,
                            EvaluationError::InvalidTransformation,
                            latency_micros,
                        );
                        let denied = check.failure_disposition == FailureDisposition::FailClosed;
                        decisions.push(decision);
                        if denied {
                            return GuardrailEvaluation {
                                action: DecisionAction::Deny,
                                decisions,
                                output: input.payload,
                            };
                        }
                        final_action = DecisionAction::Audit;
                        continue;
                    }
                    decisions.push(DecisionRecord {
                        managed_metadata: Some(metadata),
                        action: DecisionAction::Transformed,
                        reason_code,
                        transformed: true,
                        ..base_decision(
                            &input,
                            policy,
                            evaluator.id(),
                            Some(evaluator.service()),
                            latency_micros,
                            content_hash,
                        )
                    });
                    if final_action == DecisionAction::Allow {
                        final_action = DecisionAction::Transformed;
                    }
                }
                Err(error) => {
                    let decision = failure_decision(
                        evaluator.id(),
                        Some(evaluator.service()),
                        &input,
                        policy,
                        check.failure_disposition,
                        error,
                        latency_micros,
                    );
                    let denied = check.failure_disposition == FailureDisposition::FailClosed;
                    decisions.push(decision);
                    if denied {
                        return GuardrailEvaluation {
                            action: DecisionAction::Deny,
                            decisions,
                            output: input.payload,
                        };
                    }
                    final_action = DecisionAction::Audit;
                }
            }
        }

        GuardrailEvaluation {
            action: final_action,
            decisions,
            output: input.payload,
        }
    }
}

fn base_decision(
    input: &EvaluationInput,
    policy: &EffectivePolicy,
    evaluator: &str,
    managed_service: Option<ManagedService>,
    latency_micros: u64,
    content_hash: String,
) -> DecisionRecord {
    DecisionRecord {
        decision_id: DecisionId::new(),
        phase: input.phase,
        scope: policy.scope.clone(),
        evaluator: evaluator.to_string(),
        managed_service,
        managed_metadata: None,
        action: DecisionAction::Allow,
        reason_code: reason("deterministic.allow"),
        matched_rule: None,
        latency_micros,
        failure_disposition: None,
        transformed: false,
        content_hash,
    }
}

fn action_for_mode(mode: PolicyMode) -> DecisionAction {
    match mode {
        PolicyMode::Audit => DecisionAction::Audit,
        PolicyMode::Deny => DecisionAction::Deny,
    }
}

fn failure_decision(
    evaluator: &str,
    managed_service: Option<ManagedService>,
    input: &EvaluationInput,
    policy: &EffectivePolicy,
    disposition: FailureDisposition,
    error: EvaluationError,
    latency_micros: u64,
) -> DecisionRecord {
    DecisionRecord {
        action: match disposition {
            FailureDisposition::FailOpen => DecisionAction::Audit,
            FailureDisposition::FailClosed => DecisionAction::Deny,
        },
        reason_code: error.reason_code(),
        failure_disposition: Some(disposition),
        ..base_decision(
            input,
            policy,
            evaluator,
            managed_service,
            latency_micros,
            input.payload.content_hash(),
        )
    }
}

fn reason(value: &str) -> ReasonCode {
    ReasonCode::new(value).expect("static reason code is valid")
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::Arc,
        time::{Duration, Instant},
    };

    use crate::{
        BuiltInEvaluator, ContentTransformation, EffectiveScope, EvaluationPayload, GuardPhase,
        ManagedCheckConfig, ManagedCheckKind, McpCall, PackId, PolicyConfig, PolicyMode,
        test_utils::StubManagedEvaluator,
    };

    use super::*;

    fn policy(mode: PolicyMode, managed_checks: Vec<String>) -> EffectivePolicy {
        EffectivePolicy {
            enabled: true,
            mode,
            packs: vec![PackId::new("core.git").unwrap()],
            managed_checks,
            stream_buffer_bytes: 1024,
            scope: EffectiveScope::Global,
        }
    }

    fn config(names: &[&str], disposition: FailureDisposition, timeout_ms: u64) -> GuardrailConfig {
        GuardrailConfig {
            default: PolicyConfig::default(),
            managed_checks: names
                .iter()
                .map(|name| {
                    (
                        (*name).to_string(),
                        ManagedCheckConfig {
                            kind: ManagedCheckKind::AmazonBedrock,
                            phases: BTreeSet::from([
                                GuardPhase::Prompt,
                                GuardPhase::ModelResponse,
                                GuardPhase::GeneratedToolCall,
                                GuardPhase::McpCall,
                                GuardPhase::McpResult,
                                GuardPhase::HarnessPreTool,
                            ]),
                            timeout_ms,
                            failure_disposition: disposition,
                            max_content_bytes: 1024,
                            bedrock: None,
                            model_armor: None,
                        },
                    )
                })
                .collect(),
            ..GuardrailConfig::default()
        }
    }

    fn prompt(value: &str) -> EvaluationInput {
        EvaluationInput::new(
            GuardPhase::Prompt,
            EvaluationPayload::Text {
                text: value.to_string(),
            },
        )
    }

    #[tokio::test]
    async fn deterministic_deny_short_circuits_managed_checks() {
        let managed = Arc::new(StubManagedEvaluator::new(
            "aws",
            ManagedService::AmazonBedrock,
            [Ok(ManagedOutcome::Transformed {
                transformation: ContentTransformation::new("should not run".into()),
                reason_code: reason("aws.masked"),
                metadata: Default::default(),
            })],
        ));
        let engine = GuardrailEngine::new(
            vec![Arc::new(BuiltInEvaluator)],
            BTreeMap::from([("aws".to_string(), managed as Arc<dyn ManagedEvaluator>)]),
        );
        let input = EvaluationInput::new(
            GuardPhase::Prompt,
            EvaluationPayload::ShellCommand {
                command: "git reset --hard".into(),
            },
        );

        let result = engine
            .evaluate(
                &policy(PolicyMode::Deny, vec!["aws".into()]),
                &config(&["aws"], FailureDisposition::FailOpen, 100),
                input,
            )
            .await;

        assert_eq!(result.action, DecisionAction::Deny);
        assert_eq!(result.decisions.len(), 1);
        assert_eq!(result.decisions[0].evaluator, "built_in");
    }

    #[tokio::test]
    async fn audit_allows_and_managed_transformations_flow_in_order() {
        let first = Arc::new(StubManagedEvaluator::new(
            "first",
            ManagedService::AmazonBedrock,
            [Ok(ManagedOutcome::Transformed {
                transformation: ContentTransformation::new("masked".into()),
                reason_code: reason("aws.masked"),
                metadata: Default::default(),
            })],
        ));
        let second = Arc::new(StubManagedEvaluator::new(
            "second",
            ManagedService::GoogleModelArmor,
            [Ok(ManagedOutcome::Transformed {
                transformation: ContentTransformation::new("sanitized".into()),
                reason_code: reason("gcp.sanitized"),
                metadata: Default::default(),
            })],
        ));
        let engine = GuardrailEngine::new(
            vec![Arc::new(BuiltInEvaluator)],
            BTreeMap::from([
                ("first".to_string(), first as Arc<dyn ManagedEvaluator>),
                ("second".to_string(), second as Arc<dyn ManagedEvaluator>),
            ]),
        );

        let result = engine
            .evaluate(
                &policy(PolicyMode::Audit, vec!["first".into(), "second".into()]),
                &config(&["first", "second"], FailureDisposition::FailOpen, 100),
                prompt("secret"),
            )
            .await;

        assert_eq!(result.action, DecisionAction::Transformed);
        assert_eq!(
            result.output,
            EvaluationPayload::Text {
                text: "sanitized".into()
            }
        );
        assert_eq!(
            result
                .decisions
                .iter()
                .map(|decision| decision.evaluator.as_str())
                .collect::<Vec<_>>(),
            ["built_in", "first", "second"]
        );
    }

    #[tokio::test]
    async fn managed_timeout_defaults_to_fail_open_and_can_fail_closed() {
        let slow = Arc::new(
            StubManagedEvaluator::new(
                "slow",
                ManagedService::AmazonBedrock,
                [Ok(ManagedOutcome::Allow {
                    reason_code: reason("aws.allow"),
                    metadata: Default::default(),
                })],
            )
            .with_delay(Duration::from_millis(25)),
        );
        let engine = GuardrailEngine::new(
            vec![],
            BTreeMap::from([("slow".to_string(), slow as Arc<dyn ManagedEvaluator>)]),
        );

        let open = engine
            .evaluate(
                &policy(PolicyMode::Deny, vec!["slow".into()]),
                &config(&["slow"], FailureDisposition::FailOpen, 1),
                prompt("input"),
            )
            .await;
        assert_eq!(open.action, DecisionAction::Audit);
        assert_eq!(
            open.decisions[0].failure_disposition,
            Some(FailureDisposition::FailOpen)
        );

        let closed = engine
            .evaluate(
                &policy(PolicyMode::Deny, vec!["slow".into()]),
                &config(&["slow"], FailureDisposition::FailClosed, 1),
                prompt("input"),
            )
            .await;
        assert_eq!(closed.action, DecisionAction::Deny);
    }

    #[tokio::test]
    async fn managed_deny_stops_the_remaining_chain() {
        let deny = Arc::new(StubManagedEvaluator::new(
            "deny",
            ManagedService::AmazonBedrock,
            [Ok(ManagedOutcome::Intervention {
                reason_code: reason("aws.intervened"),
                metadata: Default::default(),
            })],
        ));
        let later = Arc::new(StubManagedEvaluator::new(
            "later",
            ManagedService::GoogleModelArmor,
            [Ok(ManagedOutcome::Transformed {
                transformation: ContentTransformation::new("must not run".into()),
                reason_code: reason("gcp.sanitized"),
                metadata: Default::default(),
            })],
        ));
        let engine = GuardrailEngine::new(
            vec![],
            BTreeMap::from([
                ("deny".to_string(), deny as Arc<dyn ManagedEvaluator>),
                ("later".to_string(), later as Arc<dyn ManagedEvaluator>),
            ]),
        );

        let result = engine
            .evaluate(
                &policy(PolicyMode::Deny, vec!["deny".into(), "later".into()]),
                &config(&["deny", "later"], FailureDisposition::FailOpen, 100),
                prompt("input"),
            )
            .await;

        assert_eq!(result.action, DecisionAction::Deny);
        assert_eq!(result.decisions.len(), 1);
        assert_eq!(result.decisions[0].evaluator, "deny");
        assert_eq!(result.output, prompt("input").payload);
    }

    #[tokio::test]
    async fn oversize_and_invalid_transformations_follow_failure_disposition() {
        let transform = Arc::new(StubManagedEvaluator::new(
            "transform",
            ManagedService::AmazonBedrock,
            [
                Ok(ManagedOutcome::Transformed {
                    transformation: ContentTransformation::new("not-json".into()),
                    reason_code: reason("aws.masked"),
                    metadata: Default::default(),
                }),
                Ok(ManagedOutcome::Transformed {
                    transformation: ContentTransformation::new("not-json".into()),
                    reason_code: reason("aws.masked"),
                    metadata: Default::default(),
                }),
            ],
        ));
        let engine = GuardrailEngine::new(
            vec![],
            BTreeMap::from([(
                "transform".to_string(),
                transform as Arc<dyn ManagedEvaluator>,
            )]),
        );
        let tool_input = EvaluationInput::new(
            GuardPhase::GeneratedToolCall,
            EvaluationPayload::ToolCall {
                name: "shell".into(),
                arguments: serde_json::json!({"command": "safe"}),
            },
        );
        let mut open_config = config(&["transform"], FailureDisposition::FailOpen, 100);
        open_config
            .managed_checks
            .get_mut("transform")
            .unwrap()
            .phases
            .insert(GuardPhase::GeneratedToolCall);
        let open = engine
            .evaluate(
                &policy(PolicyMode::Deny, vec!["transform".into()]),
                &open_config,
                tool_input.clone(),
            )
            .await;
        assert_eq!(open.action, DecisionAction::Audit);
        assert_eq!(
            open.decisions[0].reason_code.as_str(),
            "managed.invalid_transformation"
        );

        let mut closed_config = open_config;
        let check = closed_config.managed_checks.get_mut("transform").unwrap();
        check.failure_disposition = FailureDisposition::FailClosed;
        check.max_content_bytes = 1;
        let closed = engine
            .evaluate(
                &policy(PolicyMode::Deny, vec!["transform".into()]),
                &closed_config,
                tool_input,
            )
            .await;
        assert_eq!(closed.action, DecisionAction::Deny);
        assert_eq!(
            closed.decisions[0].reason_code.as_str(),
            "managed.content_too_large"
        );
    }
    #[tokio::test]
    async fn managed_chain_runs_once_in_order_for_every_guard_phase() {
        let outcomes = || {
            (0..6).map(|_| {
                Ok(ManagedOutcome::Allow {
                    reason_code: reason("managed.allow"),
                    metadata: Default::default(),
                })
            })
        };
        let first = Arc::new(StubManagedEvaluator::new(
            "first",
            ManagedService::AmazonBedrock,
            outcomes(),
        ));
        let second = Arc::new(StubManagedEvaluator::new(
            "second",
            ManagedService::GoogleModelArmor,
            outcomes(),
        ));
        let engine = GuardrailEngine::new(
            vec![Arc::new(BuiltInEvaluator)],
            BTreeMap::from([
                ("first".into(), first as Arc<dyn ManagedEvaluator>),
                ("second".into(), second as Arc<dyn ManagedEvaluator>),
            ]),
        );
        let inputs = [
            EvaluationInput::new(
                GuardPhase::Prompt,
                EvaluationPayload::TextSegments {
                    segments: vec!["first".into(), "second".into()],
                },
            ),
            EvaluationInput::new(
                GuardPhase::ModelResponse,
                EvaluationPayload::Text {
                    text: "response".into(),
                },
            ),
            EvaluationInput::new(
                GuardPhase::GeneratedToolCall,
                EvaluationPayload::ToolCall {
                    name: "safe".into(),
                    arguments: serde_json::json!({"value": 1}),
                },
            ),
            EvaluationInput::new(
                GuardPhase::McpCall,
                EvaluationPayload::McpCall {
                    call: McpCall {
                        server: "safe".into(),
                        tool: "read".into(),
                        arguments: serde_json::json!({"id": 1}),
                    },
                },
            ),
            EvaluationInput::new(
                GuardPhase::McpResult,
                EvaluationPayload::McpResult {
                    server: "safe".into(),
                    tool: "read".into(),
                    result: serde_json::json!({"value": 1}),
                },
            ),
            EvaluationInput::new(
                GuardPhase::HarnessPreTool,
                EvaluationPayload::ShellCommand {
                    command: "printf safe".into(),
                },
            ),
        ];
        let config = config(&["first", "second"], FailureDisposition::FailOpen, 100);
        let policy = policy(PolicyMode::Deny, vec!["first".into(), "second".into()]);

        for input in inputs {
            let result = engine.evaluate(&policy, &config, input).await;
            assert_eq!(result.action, DecisionAction::Allow);
            assert_eq!(
                result
                    .decisions
                    .iter()
                    .map(|decision| decision.evaluator.as_str())
                    .collect::<Vec<_>>(),
                ["built_in", "first", "second"]
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn managed_evaluation_load_gate_handles_bounded_concurrency_before_deadline() {
        const CONCURRENCY: usize = 64;
        let managed = Arc::new(
            StubManagedEvaluator::new(
                "managed-load",
                ManagedService::GoogleModelArmor,
                (0..CONCURRENCY).map(|_| {
                    Ok(ManagedOutcome::Allow {
                        reason_code: reason("managed.allow"),
                        metadata: Default::default(),
                    })
                }),
            )
            .with_delay(Duration::from_millis(20)),
        );
        let engine = Arc::new(GuardrailEngine::new(
            vec![Arc::new(BuiltInEvaluator)],
            BTreeMap::from([("managed-load".into(), managed as Arc<dyn ManagedEvaluator>)]),
        ));
        let policy = Arc::new(policy(PolicyMode::Deny, vec!["managed-load".into()]));
        let config = Arc::new(config(
            &["managed-load"],
            FailureDisposition::FailOpen,
            1_000,
        ));
        let started = Instant::now();
        let mut tasks = tokio::task::JoinSet::new();
        for index in 0..CONCURRENCY {
            let engine = Arc::clone(&engine);
            let policy = Arc::clone(&policy);
            let config = Arc::clone(&config);
            tasks.spawn(async move {
                engine
                    .evaluate(&policy, &config, prompt(&format!("load-{index}")))
                    .await
            });
        }
        while let Some(result) = tasks.join_next().await {
            assert_eq!(
                result.expect("managed load task").action,
                DecisionAction::Allow
            );
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{CONCURRENCY} managed evaluations took {:?}",
            started.elapsed()
        );
    }
    #[tokio::test]
    async fn associated_prompt_counts_toward_managed_content_limit() {
        let managed = Arc::new(StubManagedEvaluator::new(
            "managed",
            ManagedService::GoogleModelArmor,
            [Ok(ManagedOutcome::Allow {
                reason_code: reason("managed.allow"),
                metadata: Default::default(),
            })],
        ));
        let engine = GuardrailEngine::new(
            vec![],
            BTreeMap::from([("managed".to_string(), managed as Arc<dyn ManagedEvaluator>)]),
        );
        let input = EvaluationInput::new(
            GuardPhase::ModelResponse,
            EvaluationPayload::Text {
                text: "short".into(),
            },
        )
        .with_associated_prompt("a prompt that exceeds the configured bound");
        let mut guardrail_config = config(&["managed"], FailureDisposition::FailClosed, 100);
        guardrail_config
            .managed_checks
            .get_mut("managed")
            .unwrap()
            .max_content_bytes = 16;

        let result = engine
            .evaluate(
                &policy(PolicyMode::Deny, vec!["managed".into()]),
                &guardrail_config,
                input,
            )
            .await;

        assert_eq!(result.action, DecisionAction::Deny);
        assert_eq!(
            result.decisions[0].reason_code.as_str(),
            "managed.content_too_large"
        );
    }
}
