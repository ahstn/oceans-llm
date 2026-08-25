use super::*;

fn serialized_json_bytes<T>(value: &T) -> Option<u64>
where
    T: serde::Serialize + ?Sized,
{
    crate::payload_bounding::serialized_size(value)
        .ok()
        .and_then(|bytes| u64::try_from(bytes).ok())
}

#[derive(Debug, Clone, Copy)]
struct HarnessAdapter {
    version: &'static str,
}

fn harness_adapter(harness_key: &str) -> Option<HarnessAdapter> {
    match harness_key {
        "claude_code" => Some(HarnessAdapter {
            version: "claude-code-v1",
        }),
        "codex" => Some(HarnessAdapter {
            version: "codex-v1",
        }),
        "opencode" => Some(HarnessAdapter {
            version: "opencode-v1",
        }),
        "pi" => Some(HarnessAdapter { version: "pi-v1" }),
        "oh_my_pi" => Some(HarnessAdapter {
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
    pub(super) fn as_str(self) -> &'static str {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PassiveRequestMetadata {
    pub external_session_id: Option<String>,
    pub session_source: Option<String>,
    pub session_limitation: Option<SessionCorrelationLimitation>,
    pub execution_id: Option<String>,
    pub body_inspected: bool,
    pub parent_execution_id: Option<String>,
    pub message_count: Option<u32>,
    pub prompt_bytes: Option<u64>,
    pub supplied_tool_count: Option<u32>,
    pub tool_schema_bytes: Option<u64>,
    pub supplied_tools: Vec<BoundedToolDefinitionFact>,
    pub supplied_skills: Vec<BoundedSkillFact>,
    pub file_interactions: Vec<BoundedFileInteractionFact>,
    pub reasoning_config_hash: Option<String>,
    pub cache_requested: Option<bool>,
    pub adapter_version: String,
}

pub(crate) fn extract_request_metadata(
    body: &Value,
    headers: &BTreeMap<String, String>,
    inspect_body: bool,
    harness_key: &str,
) -> PassiveRequestMetadata {
    let body = if inspect_body { body } else { &Value::Null };
    let adapter = harness_adapter(harness_key);
    let codex_turn_metadata = if harness_key == "codex" {
        codex_turn_metadata(body, headers)
    } else {
        Vec::new()
    };
    let session = extract_session(body, headers, harness_key, &codex_turn_metadata);
    let (execution_id, parent_execution_id) =
        extract_lineage(body, headers, harness_key, &codex_turn_metadata);
    let message_count = body
        .get("messages")
        .and_then(Value::as_array)
        .or_else(|| body.get("input").and_then(Value::as_array))
        .and_then(|values| u32::try_from(values.len()).ok());
    let prompt_bytes = serialized_request_prompt_bytes(body);
    let supplied_tools = body.get("tools").and_then(Value::as_array);
    let supplied_tool_count = supplied_tools.and_then(|values| u32::try_from(values.len()).ok());
    let tool_schema_bytes = supplied_tools.and_then(serialized_json_bytes);
    let supplied_tools =
        supplied_tools.map_or_else(Vec::new, |tools| bounded_supplied_tools(tools.as_slice()));
    let instrumentation = analysis_instrumentation(body);
    let supplied_skills = instrumentation
        .and_then(|value| value.get("skills"))
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |values| bounded_skills(values));
    let file_interactions = instrumentation
        .and_then(|value| value.get("file_interactions"))
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |values| bounded_file_interactions(values));
    let reasoning_config_hash = reasoning_config_hash(body);
    let cache_requested = cache_control_requested(body);

    PassiveRequestMetadata {
        external_session_id: session.value,
        session_source: session.source,
        session_limitation: session.limitation,
        body_inspected: inspect_body,
        execution_id,
        parent_execution_id,
        message_count,
        prompt_bytes,
        supplied_tool_count,
        tool_schema_bytes,
        supplied_tools,
        supplied_skills,
        file_interactions,
        reasoning_config_hash,
        cache_requested,
        adapter_version: adapter.map_or_else(
            || "unsupported-v1".to_string(),
            |value| value.version.to_string(),
        ),
    }
}

pub(super) fn stable_uuid(namespace: Uuid, canonical: &str) -> Uuid {
    Uuid::new_v5(&namespace, canonical.as_bytes())
}

pub(super) fn hash_identifier(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("sha256:{digest:x}")
}

pub(super) fn hash_lineage_candidate(
    ownership_scope_key: &str,
    adapter_namespace: &str,
    value: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ownership_scope_key.as_bytes());
    hasher.update([0]);
    hasher.update(adapter_namespace.as_bytes());
    hasher.update([0]);
    hasher.update(b"lineage-v1");
    hasher.update([0]);
    hasher.update(value.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
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
) -> Vec<CandidateObservation> {
    headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case(expected))
        .filter_map(|(_, value)| {
            let trimmed = value.trim_matches([' ', '\t']);
            if trimmed == REDACTED_VALUE {
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

fn body_observation(body: &Value, path: &[&str], source: &str) -> Option<CandidateObservation> {
    let mut current = body;
    for segment in path {
        current = current.get(*segment)?;
    }
    let Some(value) = current.as_str() else {
        return Some(CandidateObservation::Invalid);
    };
    if value == REDACTED_VALUE {
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

fn metadata_observation(metadata: &EmbeddedMetadata, key: &str) -> Option<CandidateObservation> {
    let value = metadata.value.get(key)?;
    let Some(value) = value.as_str() else {
        return Some(CandidateObservation::Invalid);
    };
    if value == REDACTED_VALUE {
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
    body: &Value,
    headers: &BTreeMap<String, String>,
    harness_key: &str,
    codex_metadata: &[EmbeddedMetadata],
) -> SessionResolution {
    match harness_key {
        "claude_code" => resolve_session(header_observations(headers, "x-claude-code-session-id")),
        "codex" => {
            let mut observations = header_observations(headers, "session-id");
            observations.extend(body_observation(
                body,
                &["client_metadata", "session_id"],
                "body:client_metadata.session_id",
            ));
            observations.extend(
                codex_metadata
                    .iter()
                    .filter_map(|metadata| metadata_observation(metadata, "session_id")),
            );
            resolve_session(observations)
        }
        "opencode" => extract_opencode_session(headers),
        "pi" => extract_pi_session(headers),
        "oh_my_pi" => extract_oh_my_pi_session(body, headers),
        _ => SessionResolution::default(),
    }
}

fn extract_opencode_session(headers: &BTreeMap<String, String>) -> SessionResolution {
    let mut v1 = header_observations(headers, "x-session-id");
    v1.extend(header_observations(headers, "x-session-affinity"));
    let managed = header_observations(headers, "x-opencode-session");
    if !v1.is_empty() && !managed.is_empty() {
        return SessionResolution::conflicted();
    }
    if managed.is_empty() {
        resolve_session(v1)
    } else {
        resolve_session(managed)
    }
}

fn extract_pi_session(headers: &BTreeMap<String, String>) -> SessionResolution {
    let canonical = header_observations(headers, "session_id");
    let corroborating = header_observations(headers, "x-client-request-id");
    if canonical.is_empty() {
        return SessionResolution::default();
    }
    let mut observations = canonical;
    observations.extend(corroborating);
    resolve_session(observations)
}

fn extract_oh_my_pi_session(body: &Value, headers: &BTreeMap<String, String>) -> SessionResolution {
    let mut observations = header_observations(headers, "x-claude-code-session-id");
    observations.extend(header_observations(headers, "session_id"));
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
            None if user_id.as_str() == Some(REDACTED_VALUE) => {}
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

fn codex_turn_metadata(body: &Value, headers: &BTreeMap<String, String>) -> Vec<EmbeddedMetadata> {
    let mut result = Vec::new();
    if let Some(value) = body
        .get("client_metadata")
        .and_then(|metadata| metadata.get("x-codex-turn-metadata"))
        .and_then(Value::as_str)
        .and_then(parse_bounded_json_object)
    {
        result.push(EmbeddedMetadata {
            value,
            source: "body:client_metadata.x-codex-turn-metadata".to_string(),
        });
    }
    for (_, raw) in headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("x-codex-turn-metadata"))
    {
        let raw = raw.trim_matches([' ', '\t']);
        if let Some(value) = parse_bounded_json_object(raw) {
            result.push(EmbeddedMetadata {
                value,
                source: "header:x-codex-turn-metadata".to_string(),
            });
        }
    }
    result
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
    body: &Value,
    headers: &BTreeMap<String, String>,
    harness_key: &str,
    codex_metadata: &[EmbeddedMetadata],
) -> (Option<String>, Option<String>) {
    match harness_key {
        "claude_code" => (
            resolved_lineage_value(header_observations(headers, "x-claude-code-agent-id")),
            resolved_lineage_value(header_observations(
                headers,
                "x-claude-code-parent-agent-id",
            )),
        ),
        "opencode" => (
            None,
            resolved_lineage_value(header_observations(headers, "x-parent-session-id")),
        ),
        "codex" => extract_codex_lineage(body, headers, codex_metadata),
        _ => (None, None),
    }
}

fn extract_codex_lineage(
    body: &Value,
    headers: &BTreeMap<String, String>,
    metadata: &[EmbeddedMetadata],
) -> (Option<String>, Option<String>) {
    let mut thread = header_observations(headers, "thread-id");
    thread.extend(header_observations(headers, "x-client-request-id"));
    thread.extend(body_observation(
        body,
        &["client_metadata", "thread_id"],
        "body:client_metadata.thread_id",
    ));
    thread.extend(
        metadata
            .iter()
            .filter_map(|value| metadata_observation(value, "thread_id")),
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
                .filter_map(|value| metadata_observation(value, "turn_id")),
        );
        resolved_lineage_value(turn)
    };

    let mut parent = metadata
        .iter()
        .filter_map(|value| metadata_observation(value, "parent_thread_id"))
        .collect::<Vec<_>>();
    if parent.is_empty() {
        parent.extend(
            metadata
                .iter()
                .filter_map(|value| metadata_observation(value, "forked_from_thread_id")),
        );
    }
    (execution_id, resolved_lineage_value(parent))
}

pub(crate) fn serialized_request_prompt_bytes(body: &Value) -> Option<u64> {
    let primary_prompt = body.get("messages").or_else(|| body.get("input"));
    let mut total = 0_u64;
    let mut found = false;
    for prompt in [body.get("instructions"), primary_prompt]
        .into_iter()
        .flatten()
        .filter(|prompt| !prompt.is_null())
    {
        found = true;
        total = total.checked_add(serialized_json_bytes(prompt)?)?;
    }
    found.then_some(total)
}

fn bounded_supplied_tools(tools: &[Value]) -> Vec<BoundedToolDefinitionFact> {
    tools
        .iter()
        .take(MAX_SUPPLIED_TOOL_FACTS)
        .filter_map(|tool| {
            let name = tool
                .pointer("/function/name")
                .or_else(|| tool.get("name"))
                .and_then(Value::as_str)?
                .chars()
                .take(MAX_TOOL_NAME_CHARS)
                .collect::<String>();
            if name.is_empty() {
                return None;
            }
            let token_estimate = serialized_json_bytes(tool)?.div_ceil(4);
            Some(BoundedToolDefinitionFact {
                server_key: tool_server_key(&name),
                name,
                token_estimate,
            })
        })
        .collect()
}

fn analysis_instrumentation(body: &Value) -> Option<&serde_json::Map<String, Value>> {
    body.pointer("/metadata/agent_analysis")
        .or_else(|| body.pointer("/metadata/oceans_agent_analysis"))
        .and_then(Value::as_object)
}

fn bounded_skills(values: &[Value]) -> Vec<BoundedSkillFact> {
    values
        .iter()
        .take(MAX_SKILL_FACTS)
        .filter_map(|value| {
            let value = value.as_object()?;
            let name = value
                .get("name")?
                .as_str()?
                .chars()
                .take(MAX_TOOL_NAME_CHARS)
                .collect::<String>();
            (!name.is_empty()).then(|| BoundedSkillFact {
                name,
                description_token_estimate: bounded_u64(value.get("description_tokens")),
                body_token_estimate: bounded_u64(value.get("body_tokens")),
                resource_token_estimate: bounded_u64(value.get("resource_tokens")),
                used: value.get("used").and_then(Value::as_bool).unwrap_or(false),
                abandoned: value.get("abandoned").and_then(Value::as_bool),
            })
        })
        .collect()
}

fn bounded_file_interactions(values: &[Value]) -> Vec<BoundedFileInteractionFact> {
    values
        .iter()
        .take(MAX_FILE_INTERACTION_FACTS)
        .filter_map(|value| {
            let value = value.as_object()?;
            let opaque_file_id = value
                .get("opaque_file_id")?
                .as_str()?
                .chars()
                .take(MAX_TOOL_NAME_CHARS)
                .collect::<String>();
            let operation = value
                .get("operation")?
                .as_str()?
                .to_ascii_lowercase()
                .chars()
                .take(32)
                .collect::<String>();
            if opaque_file_id.is_empty()
                || !matches!(
                    operation.as_str(),
                    "read" | "search" | "create" | "edit" | "overwrite" | "verify"
                )
            {
                return None;
            }
            Some(BoundedFileInteractionFact {
                opaque_file_id,
                operation,
                tool_name: value
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .map(|name| name.chars().take(MAX_TOOL_NAME_CHARS).collect()),
                succeeded: value.get("succeeded").and_then(Value::as_bool),
                error_signature: value
                    .get("error_code")
                    .and_then(Value::as_str)
                    .map(|code| code.chars().take(MAX_TOOL_NAME_CHARS).collect()),
            })
        })
        .collect()
}

fn bounded_u64(value: Option<&Value>) -> Option<u64> {
    value
        .and_then(Value::as_u64)
        .filter(|value| *value <= 10_000_000)
}

fn reasoning_config_hash(body: &Value) -> Option<String> {
    let value = body
        .get("reasoning")
        .or_else(|| body.get("reasoning_effort"))
        .or_else(|| body.get("thinking"))?;
    serde_json::to_string(value)
        .ok()
        .map(|value| hash_identifier(&value))
}

fn cache_control_requested(body: &Value) -> Option<bool> {
    let mut remaining = 2_048;
    let requested = body.get("cache_control").is_some()
        || body.get("prompt_cache_options").is_some()
        || contains_cache_control(body, 0, &mut remaining);
    requested.then_some(true)
}

fn contains_cache_control(value: &Value, depth: usize, remaining: &mut usize) -> bool {
    if depth > 16 || *remaining == 0 {
        return false;
    }
    *remaining -= 1;
    match value {
        Value::Object(values) => values.iter().any(|(key, value)| {
            matches!(
                key.as_str(),
                "cache_control" | "cachePoint" | "prompt_cache_breakpoint"
            ) || contains_cache_control(value, depth + 1, remaining)
        }),
        Value::Array(values) => values
            .iter()
            .any(|value| contains_cache_control(value, depth + 1, remaining)),
        _ => false,
    }
}

pub(super) fn tool_server_key(name: &str) -> Option<String> {
    if let Some(value) = name.strip_prefix("mcp__") {
        return value
            .split_once("__")
            .map(|(server, _)| server.to_string())
            .filter(|value| !value.is_empty());
    }
    ['.', '/']
        .into_iter()
        .find_map(|delimiter| name.split_once(delimiter).map(|(server, _)| server))
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}
