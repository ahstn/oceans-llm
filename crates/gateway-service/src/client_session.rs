//! Bounded client session and lineage identifiers shared by routing and passive analysis.

use std::collections::BTreeMap;

use gateway_core::GatewayError;
use serde_json::Value;

use crate::redaction::REDACTED_VALUE;

pub(crate) const MAX_EXTERNAL_IDENTIFIER_BYTES: usize = 256;
const MAX_TURN_METADATA_BYTES: usize = 4_096;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ParsePolicy {
    Passive,
    Routing,
}

/// Only these extension fields can identify a session. Prompt content is never visited.
#[derive(Clone, Copy)]
struct SessionFields<'a> {
    session_id: Option<&'a Value>,
    client_metadata: Option<&'a Value>,
    metadata: Option<&'a Value>,
    policy: ParsePolicy,
}

impl<'a> SessionFields<'a> {
    fn new(get: impl Fn(&str) -> Option<&'a Value>, policy: ParsePolicy) -> Self {
        Self {
            session_id: get("session_id"),
            client_metadata: get("client_metadata"),
            metadata: get("metadata"),
            policy,
        }
    }

    fn get(self, key: &str) -> Option<&'a Value> {
        match key {
            "session_id" => self.session_id,
            "client_metadata" => self.client_metadata,
            "metadata" => self.metadata,
            _ => None,
        }
    }
}

pub(crate) struct ClientSessionMetadata {
    pub session: SessionResolution,
    pub execution_id: Option<String>,
    pub parent_execution_id: Option<String>,
    pub adapter_version: &'static str,
}

pub(crate) fn extract_client_session(
    body: &Value,
    headers: &BTreeMap<String, String>,
    harness_key: &str,
) -> ClientSessionMetadata {
    let body = SessionFields::new(|key| body.get(key), ParsePolicy::Passive);
    let metadata = if harness_key == "codex" {
        codex_turn_metadata(body, headers).0
    } else {
        Vec::new()
    };
    let session = extract_session(body, headers, harness_key, &metadata);
    let (execution_id, parent_execution_id) =
        extract_lineage(body, headers, harness_key, &metadata);
    ClientSessionMetadata {
        session,
        execution_id,
        parent_execution_id,
        adapter_version: harness_adapter(harness_key)
            .map_or("unsupported-v1", |adapter| adapter.version),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RoutingSession {
    pub namespace: &'static str,
    pub value: String,
}

/// The canonical header selects the Oceans namespace, but must agree with any recognized
/// harness session identifier. Unlike passive analysis, malformed IDs reject routing.
pub(crate) fn extract_routing_session(
    extra: &BTreeMap<String, Value>,
    headers: &BTreeMap<String, String>,
    harness_key: &str,
) -> Result<Option<RoutingSession>, GatewayError> {
    let body = SessionFields::new(|key| extra.get(key), ParsePolicy::Routing);
    let (metadata, malformed) = if harness_key == "codex" {
        codex_turn_metadata(body, headers)
    } else {
        (Vec::new(), false)
    };
    if malformed {
        return Err(invalid_routing_session(
            SessionCorrelationLimitation::MalformedCandidate,
        ));
    }
    let canonical = resolve_session(header_observations(
        headers,
        "x-oceans-session-id",
        body.policy,
    ));
    let harness = extract_session(body, headers, harness_key, &metadata);
    if let Some(limitation) = canonical.limitation.or(harness.limitation) {
        return Err(invalid_routing_session(limitation));
    }
    if let (Some(canonical), Some(harness)) = (&canonical.value, &harness.value)
        && canonical != harness
    {
        return Err(invalid_routing_session(
            SessionCorrelationLimitation::ConflictingAliases,
        ));
    }
    if let Some(value) = canonical.value {
        return Ok(Some(RoutingSession {
            namespace: "oceans",
            value,
        }));
    }
    let Some(adapter) = harness_adapter(harness_key) else {
        return Ok(None);
    };
    Ok(harness.value.map(|value| RoutingSession {
        namespace: adapter.namespace,
        value,
    }))
}

fn invalid_routing_session(limitation: SessionCorrelationLimitation) -> GatewayError {
    let message = match limitation {
        SessionCorrelationLimitation::ConflictingAliases => {
            "conflicting routing session identifiers"
        }
        SessionCorrelationLimitation::MalformedCandidate => {
            "invalid routing session identifier: expected 1 to 256 ASCII letters, digits, '.', '_', ':', or '-' and valid bounded harness metadata"
        }
    };
    GatewayError::InvalidRequest(message.to_string())
}

#[derive(Debug, Clone, Copy)]
struct HarnessAdapter {
    namespace: &'static str,
    version: &'static str,
}

fn harness_adapter(harness_key: &str) -> Option<HarnessAdapter> {
    match harness_key {
        "claude_code" => Some(HarnessAdapter {
            namespace: "claude_code",
            version: "claude-code-v1",
        }),
        "codex" => Some(HarnessAdapter {
            namespace: "codex",
            version: "codex-v1",
        }),
        "opencode" => Some(HarnessAdapter {
            namespace: "opencode",
            version: "opencode-v1",
        }),
        "pi" => Some(HarnessAdapter {
            namespace: "pi",
            version: "pi-v1",
        }),
        "oh_my_pi" => Some(HarnessAdapter {
            namespace: "oh_my_pi",
            version: "oh-my-pi-v1",
        }),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionCorrelationLimitation {
    ConflictingAliases,
    MalformedCandidate,
}

impl SessionCorrelationLimitation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ConflictingAliases => "conflicting_aliases",
            Self::MalformedCandidate => "malformed_candidate",
        }
    }
}

#[derive(Debug)]
enum CandidateObservation {
    Valid { value: String, source: String },
    Invalid,
}

#[derive(Debug, Default)]
pub(crate) struct SessionResolution {
    pub value: Option<String>,
    pub source: Option<String>,
    pub limitation: Option<SessionCorrelationLimitation>,
}

impl SessionResolution {
    fn conflicted() -> Self {
        Self {
            limitation: Some(SessionCorrelationLimitation::ConflictingAliases),
            ..Self::default()
        }
    }
}

#[derive(Debug)]
struct EmbeddedMetadata {
    value: Value,
    source: String,
}

fn normalized_identifier(value: &str, trim_http_ows: bool) -> Option<String> {
    let value = if trim_http_ows {
        value.trim_matches([' ', '\t'])
    } else {
        value
    };
    if value.is_empty()
        || value == REDACTED_VALUE
        || value.len() > MAX_EXTERNAL_IDENTIFIER_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return None;
    }
    Some(value.to_string())
}

fn header_observations(
    headers: &BTreeMap<String, String>,
    expected: &str,
    policy: ParsePolicy,
) -> Vec<CandidateObservation> {
    headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case(expected))
        .filter_map(|(_, value)| {
            let trimmed = value.trim_matches([' ', '\t']);
            if trimmed == REDACTED_VALUE && policy == ParsePolicy::Passive {
                return None;
            }
            Some(normalized_identifier(value, true).map_or(
                CandidateObservation::Invalid,
                |value| CandidateObservation::Valid {
                    value,
                    source: format!("header:{expected}"),
                },
            ))
        })
        .collect()
}

fn body_observation(
    body: SessionFields<'_>,
    path: &[&str],
    source: &str,
) -> Option<CandidateObservation> {
    let (first, remaining) = path.split_first()?;
    let mut current = body.get(first)?;
    for segment in remaining {
        current = current.get(*segment)?;
    }
    let Some(value) = current.as_str() else {
        return Some(CandidateObservation::Invalid);
    };
    if value == REDACTED_VALUE && body.policy == ParsePolicy::Passive {
        return None;
    }
    Some(
        normalized_identifier(value, false).map_or(CandidateObservation::Invalid, |value| {
            CandidateObservation::Valid {
                value,
                source: source.to_string(),
            }
        }),
    )
}

fn metadata_observation(
    metadata: &EmbeddedMetadata,
    key: &str,
    policy: ParsePolicy,
) -> Option<CandidateObservation> {
    let value = metadata.value.get(key)?;
    let Some(value) = value.as_str() else {
        return Some(CandidateObservation::Invalid);
    };
    if value == REDACTED_VALUE && policy == ParsePolicy::Passive {
        return None;
    }
    Some(
        normalized_identifier(value, false).map_or(CandidateObservation::Invalid, |value| {
            CandidateObservation::Valid {
                value,
                source: format!("{}.{}", metadata.source, key),
            }
        }),
    )
}

fn resolve_session(observations: Vec<CandidateObservation>) -> SessionResolution {
    let mut accepted: Vec<(String, String)> = Vec::new();
    let mut invalid = false;
    for observation in observations {
        match observation {
            CandidateObservation::Valid { value, source } => accepted.push((value, source)),
            CandidateObservation::Invalid => invalid = true,
        }
    }
    if accepted
        .iter()
        .skip(1)
        .any(|(value, _)| value != &accepted[0].0)
    {
        return SessionResolution::conflicted();
    }
    if invalid {
        return SessionResolution {
            limitation: Some(SessionCorrelationLimitation::MalformedCandidate),
            ..SessionResolution::default()
        };
    }
    let Some((value, _)) = accepted.first() else {
        return SessionResolution::default();
    };
    let value = value.clone();
    let mut sources = Vec::new();
    for (_, source) in accepted {
        if !sources.contains(&source) {
            sources.push(source);
        }
    }
    SessionResolution {
        value: Some(value),
        source: Some(sources.join("+")),
        limitation: None,
    }
}

fn extract_session(
    body: SessionFields<'_>,
    headers: &BTreeMap<String, String>,
    harness_key: &str,
    codex_metadata: &[EmbeddedMetadata],
) -> SessionResolution {
    match harness_key {
        "claude_code" => resolve_session(header_observations(
            headers,
            "x-claude-code-session-id",
            body.policy,
        )),
        "codex" => {
            let mut observations = header_observations(headers, "session-id", body.policy);
            observations.extend(body_observation(
                body,
                &["client_metadata", "session_id"],
                "body:client_metadata.session_id",
            ));
            observations.extend(
                codex_metadata.iter().filter_map(|metadata| {
                    metadata_observation(metadata, "session_id", body.policy)
                }),
            );
            resolve_session(observations)
        }
        "opencode" => extract_opencode_session(body, headers),
        "pi" => extract_pi_session(body, headers),
        "oh_my_pi" => extract_oh_my_pi_session(body, headers),
        _ => SessionResolution::default(),
    }
}

fn extract_opencode_session(
    body: SessionFields<'_>,
    headers: &BTreeMap<String, String>,
) -> SessionResolution {
    let mut v1 = header_observations(headers, "x-session-id", body.policy);
    v1.extend(header_observations(
        headers,
        "x-session-affinity",
        body.policy,
    ));
    let managed = header_observations(headers, "x-opencode-session", body.policy);
    if !v1.is_empty() && !managed.is_empty() {
        return SessionResolution::conflicted();
    }
    if managed.is_empty() {
        resolve_session(v1)
    } else {
        resolve_session(managed)
    }
}

fn extract_pi_session(
    body: SessionFields<'_>,
    headers: &BTreeMap<String, String>,
) -> SessionResolution {
    let canonical = header_observations(headers, "session_id", body.policy);
    let corroborating = header_observations(headers, "x-client-request-id", body.policy);
    if canonical.is_empty() {
        return SessionResolution::default();
    }
    let mut observations = canonical;
    observations.extend(corroborating);
    resolve_session(observations)
}

fn extract_oh_my_pi_session(
    body: SessionFields<'_>,
    headers: &BTreeMap<String, String>,
) -> SessionResolution {
    let mut observations = header_observations(headers, "x-claude-code-session-id", body.policy);
    observations.extend(header_observations(headers, "session_id", body.policy));
    observations.extend(body_observation(body, &["session_id"], "body:session_id"));
    if let Some(user_id) = body
        .get("metadata")
        .and_then(|metadata| metadata.get("user_id"))
    {
        match user_id.as_str().and_then(parse_bounded_json_object) {
            Some(metadata) => observations.extend(
                metadata
                    .get("session_id")
                    .map(|value| {
                        value
                            .as_str()
                            .and_then(|value| normalized_identifier(value, false))
                    })
                    .map(|value| {
                        value.map_or(CandidateObservation::Invalid, |value| {
                            CandidateObservation::Valid {
                                value,
                                source: "body:metadata.user_id.session_id".to_string(),
                            }
                        })
                    }),
            ),
            None if user_id.as_str() == Some(REDACTED_VALUE)
                && body.policy == ParsePolicy::Passive => {}
            None => observations.push(CandidateObservation::Invalid),
        }
    }
    resolve_session(observations)
}

fn parse_bounded_json_object(value: &str) -> Option<Value> {
    if value.len() > MAX_TURN_METADATA_BYTES {
        return None;
    }
    let parsed: Value = serde_json::from_str(value).ok()?;
    parsed.is_object().then_some(parsed)
}

fn codex_turn_metadata(
    body: SessionFields<'_>,
    headers: &BTreeMap<String, String>,
) -> (Vec<EmbeddedMetadata>, bool) {
    let mut result = Vec::new();
    let mut malformed = false;
    let body_value = body
        .get("client_metadata")
        .and_then(|metadata| metadata.get("x-codex-turn-metadata"));
    if let Some(raw) = body_value {
        if let Some(value) = raw.as_str().and_then(parse_bounded_json_object) {
            result.push(EmbeddedMetadata {
                value,
                source: "body:client_metadata.x-codex-turn-metadata".to_string(),
            });
        } else {
            malformed = true;
        }
    }
    for (_, raw) in headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("x-codex-turn-metadata"))
    {
        if let Some(value) = parse_bounded_json_object(raw.trim_matches([' ', '\t'])) {
            result.push(EmbeddedMetadata {
                value,
                source: "header:x-codex-turn-metadata".to_string(),
            });
        } else {
            malformed = true;
        }
    }
    (result, malformed)
}

fn resolved_lineage_value(observations: Vec<CandidateObservation>) -> Option<String> {
    let resolution = resolve_session(observations);
    if resolution.limitation.is_none() {
        resolution.value
    } else {
        None
    }
}

fn extract_lineage(
    body: SessionFields<'_>,
    headers: &BTreeMap<String, String>,
    harness_key: &str,
    codex_metadata: &[EmbeddedMetadata],
) -> (Option<String>, Option<String>) {
    match harness_key {
        "claude_code" => (
            resolved_lineage_value(header_observations(
                headers,
                "x-claude-code-agent-id",
                body.policy,
            )),
            resolved_lineage_value(header_observations(
                headers,
                "x-claude-code-parent-agent-id",
                body.policy,
            )),
        ),
        "opencode" => (
            None,
            resolved_lineage_value(header_observations(
                headers,
                "x-parent-session-id",
                body.policy,
            )),
        ),
        "codex" => extract_codex_lineage(body, headers, codex_metadata),
        _ => (None, None),
    }
}

fn extract_codex_lineage(
    body: SessionFields<'_>,
    headers: &BTreeMap<String, String>,
    metadata: &[EmbeddedMetadata],
) -> (Option<String>, Option<String>) {
    let mut thread = header_observations(headers, "thread-id", body.policy);
    thread.extend(header_observations(
        headers,
        "x-client-request-id",
        body.policy,
    ));
    thread.extend(body_observation(
        body,
        &["client_metadata", "thread_id"],
        "body:client_metadata.thread_id",
    ));
    thread.extend(
        metadata
            .iter()
            .filter_map(|value| metadata_observation(value, "thread_id", body.policy)),
    );
    let thread = resolve_session(thread);
    let execution_id = if thread.limitation.is_some() {
        None
    } else if thread.value.is_some() {
        thread.value
    } else {
        let mut turn = body_observation(
            body,
            &["client_metadata", "turn_id"],
            "body:client_metadata.turn_id",
        )
        .into_iter()
        .collect::<Vec<_>>();
        turn.extend(
            metadata
                .iter()
                .filter_map(|value| metadata_observation(value, "turn_id", body.policy)),
        );
        resolved_lineage_value(turn)
    };

    let mut parent = metadata
        .iter()
        .filter_map(|value| metadata_observation(value, "parent_thread_id", body.policy))
        .collect::<Vec<_>>();
    if parent.is_empty() {
        parent.extend(
            metadata.iter().filter_map(|value| {
                metadata_observation(value, "forked_from_thread_id", body.policy)
            }),
        );
    }
    (execution_id, resolved_lineage_value(parent))
}

#[cfg(test)]
mod tests;
