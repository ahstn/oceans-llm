use super::*;

fn serialized_json_bytes<T>(value: &T) -> Option<u64>
where
    T: serde::Serialize + ?Sized,
{
    crate::payload_bounding::serialized_size(value)
        .ok()
        .and_then(|bytes| u64::try_from(bytes).ok())
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
    let client = crate::client_session::extract_client_session(body, headers, harness_key);
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
        external_session_id: client.session.value,
        session_source: client.session.source,
        session_limitation: client.session.limitation,
        body_inspected: inspect_body,
        execution_id: client.execution_id,
        parent_execution_id: client.parent_execution_id,
        message_count,
        prompt_bytes,
        supplied_tool_count,
        tool_schema_bytes,
        supplied_tools,
        supplied_skills,
        file_interactions,
        reasoning_config_hash,
        cache_requested,
        adapter_version: client.adapter_version.to_string(),
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
