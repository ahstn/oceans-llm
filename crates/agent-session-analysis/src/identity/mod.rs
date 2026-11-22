//! Bounded client session and lineage identifiers shared by routing and passive analysis.

use std::collections::BTreeMap;

use serde_json::Value;

mod adapters;
pub mod harness;

use adapters::{codex_turn_metadata, extract_lineage, extract_session};

/// Shared wire marker written by service redaction and recognized by identity parsing.
pub const REDACTED_VALUE: &str = "[REDACTED]";

/// Maximum accepted length of an external session or lineage identifier, in bytes.
pub const MAX_EXTERNAL_IDENTIFIER_BYTES: usize = 256;

/// Headers validated for duplicate and non-text values during routing.
pub const ROUTING_SESSION_HEADERS: &[&str] = &[
    "x-oceans-session-id",
    "session-id",
    "session_id",
    "x-claude-code-session-id",
    "x-session-id",
    "x-session-affinity",
    "x-opencode-session",
    "x-opencode-session-id",
    "x-client-request-id",
    "x-codex-turn-metadata",
    "x-litellm-session-id",
];

/// Headers retained for diagnostics; unlike routing validation these include lineage headers.
pub const SESSION_ANALYSIS_DIAGNOSTIC_HEADERS: &[&str] = &[
    "session-id",
    "session_id",
    "thread-id",
    "x-claude-code-agent-id",
    "x-claude-code-parent-agent-id",
    "x-claude-code-session-id",
    "x-client-request-id",
    "x-codex-parent-thread-id",
    "x-codex-turn-metadata",
    "x-litellm-session-id",
    "x-oceans-session-id",
    "x-opencode-parent-session-id",
    "x-opencode-session",
    "x-opencode-session-id",
    "x-parent-session-id",
    "x-session-affinity",
    "x-session-id",
];
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

pub struct ClientSessionMetadata {
    pub session_value: Option<String>,
    pub session_source: Option<String>,
    pub session_limitation: Option<SessionCorrelationLimitation>,
    /// Namespace `session_value` is hashed under: [`OCEANS_SESSION_NAMESPACE`] when the canonical
    /// header names the session, otherwise the harness adapter's. `None` without a session.
    pub session_namespace: Option<&'static str>,
    pub execution_id: Option<String>,
    pub parent_execution_id: Option<String>,
    /// Namespace the lineage IDs are hashed under: the harness adapter's, even when the
    /// canonical header names the session. `None` without an adapter, which reads no lineage.
    pub lineage_namespace: Option<&'static str>,
    pub adapter_version: &'static str,
}

/// Passive analysis never rejects a request: an unusable session ID becomes a limitation.
pub fn extract_client_session(
    body: &Value,
    headers: &BTreeMap<String, String>,
    harness_key: &str,
) -> ClientSessionMetadata {
    let body = SessionFields::new(|key| body.get(key), ParsePolicy::Passive);
    let adapter = harness_adapter(harness_key);
    let metadata = if adapter == Some(HarnessAdapter::Codex) {
        codex_turn_metadata(body, headers).0
    } else {
        Vec::new()
    };
    let (session, session_namespace) = select_session(
        canonical_session(body, headers),
        extract_session(body, headers, adapter, &metadata),
        adapter,
    );
    let (execution_id, parent_execution_id) = extract_lineage(body, headers, adapter, &metadata);
    ClientSessionMetadata {
        session_value: session.value,
        session_source: session.source,
        session_limitation: session.limitation,
        session_namespace,
        execution_id,
        parent_execution_id,
        lineage_namespace: adapter.map(HarnessAdapter::namespace),
        adapter_version: adapter.map_or("unsupported-v1", HarnessAdapter::version),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct RoutingSession {
    pub namespace: &'static str,
    pub value: String,
}

/// Unlike passive analysis, an unusable session ID rejects routing.
pub fn extract_routing_session(
    extra: &BTreeMap<String, Value>,
    headers: &BTreeMap<String, String>,
    harness_key: &str,
) -> Result<Option<RoutingSession>, SessionCorrelationLimitation> {
    let body = SessionFields::new(|key| extra.get(key), ParsePolicy::Routing);
    let adapter = harness_adapter(harness_key);
    let (metadata, malformed) = if adapter == Some(HarnessAdapter::Codex) {
        codex_turn_metadata(body, headers)
    } else {
        (Vec::new(), false)
    };
    if malformed {
        return Err(SessionCorrelationLimitation::MalformedCandidate);
    }
    let (session, namespace) = select_session(
        canonical_session(body, headers),
        extract_session(body, headers, adapter, &metadata),
        adapter,
    );
    if let Some(limitation) = session.limitation {
        return Err(limitation);
    }
    Ok(session
        .value
        .zip(namespace)
        .map(|(value, namespace)| RoutingSession { namespace, value }))
}

/// The canonical header names the session in [`OCEANS_SESSION_NAMESPACE`], but must agree with
/// any session ID the harness adapter found. Without the canonical header, the adapter's ID names
/// the session in the adapter's namespace. A limitation on either side, or a disagreement, leaves
/// no session. Routing and passive analysis share this rule.
fn select_session(
    canonical: SessionResolution,
    harness: SessionResolution,
    adapter: Option<HarnessAdapter>,
) -> (SessionResolution, Option<&'static str>) {
    if let Some(limitation) = canonical.limitation.or(harness.limitation) {
        return (
            SessionResolution {
                limitation: Some(limitation),
                ..SessionResolution::default()
            },
            None,
        );
    }
    let Some(value) = canonical.value else {
        // `extract_session` finds a value only through an adapter.
        let namespace = harness
            .value
            .as_ref()
            .and(adapter)
            .map(HarnessAdapter::namespace);
        return (harness, namespace);
    };
    if harness.value.as_ref().is_some_and(|other| *other != value) {
        return (SessionResolution::conflicted(), None);
    }
    let source = match (canonical.source, harness.source) {
        (Some(canonical_source), Some(harness_source)) => {
            Some(format!("{canonical_source}+{harness_source}"))
        }
        (canonical_source, harness_source) => canonical_source.or(harness_source),
    };
    let session = SessionResolution {
        value: Some(value),
        source,
        limitation: None,
    };
    (session, Some(OCEANS_SESSION_NAMESPACE))
}

fn canonical_session(
    body: SessionFields<'_>,
    headers: &BTreeMap<String, String>,
) -> SessionResolution {
    resolve_session(header_observations(
        headers,
        OCEANS_SESSION_HEADER,
        body.policy,
    ))
}

/// A harness whose own session and lineage IDs the gateway reads. Several harness keys can
/// share one adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HarnessAdapter {
    ClaudeCode,
    Codex,
    OpenCode,
    Pi,
    OhMyPi,
}

impl HarnessAdapter {
    /// Namespace the adapter's session and lineage IDs are hashed and routed under.
    const fn namespace(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Pi => "pi",
            Self::OhMyPi => "oh_my_pi",
        }
    }

    fn version(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code-v2",
            Self::Codex => "codex-v1",
            Self::OpenCode => "opencode-v2",
            Self::Pi => "pi-v2",
            Self::OhMyPi => "oh-my-pi-v2",
        }
    }
}

/// Canonical session header that any client can send, whatever its harness.
const OCEANS_SESSION_HEADER: &str = "x-oceans-session-id";

/// Namespace of sessions named by the canonical [`OCEANS_SESSION_HEADER`].
pub const OCEANS_SESSION_NAMESPACE: &str = "oceans";

/// Every namespace a passive session hash can be computed under: each harness adapter's,
/// plus the canonical header's. A caller that knows a raw session id but not which harness
/// the gateway classified its traffic as matches against all of them.
pub const SESSION_HASH_NAMESPACES: [&str; 6] = [
    HarnessAdapter::ClaudeCode.namespace(),
    HarnessAdapter::Codex.namespace(),
    HarnessAdapter::OpenCode.namespace(),
    HarnessAdapter::Pi.namespace(),
    HarnessAdapter::OhMyPi.namespace(),
    OCEANS_SESSION_NAMESPACE,
];

fn harness_adapter(harness_key: &str) -> Option<HarnessAdapter> {
    match harness_key {
        // Claude Code identifies itself as `claude-cli/`, as does OMP's Messages client, which
        // sends the same Claude Code session header.
        "claude_code" | "claude_cli" => Some(HarnessAdapter::ClaudeCode),
        "codex" => Some(HarnessAdapter::Codex),
        "opencode" => Some(HarnessAdapter::OpenCode),
        "pi" => Some(HarnessAdapter::Pi),
        "oh_my_pi" => Some(HarnessAdapter::OhMyPi),
        _ => None,
    }
}

/// Whether `value` is a session id the gateway would accept from a request.
pub fn is_external_session_id(value: &str) -> bool {
    normalized_identifier(value, false).is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCorrelationLimitation {
    ConflictingAliases,
    MalformedCandidate,
}

impl std::fmt::Display for SessionCorrelationLimitation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ConflictingAliases => "conflicting routing session identifiers",
            Self::MalformedCandidate => "invalid routing session identifier: expected 1 to 256 ASCII letters, digits, '.', '_', ':', or '-' and valid bounded harness metadata",
        })
    }
}

impl SessionCorrelationLimitation {
    pub fn as_str(self) -> &'static str {
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
struct SessionResolution {
    value: Option<String>,
    source: Option<String>,
    limitation: Option<SessionCorrelationLimitation>,
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

#[cfg(test)]
mod tests;
