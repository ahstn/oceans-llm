use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use gateway_core::ProviderError;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub(crate) fn stable_prefixed_hash(prefix: &str, value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("{prefix}{}", URL_SAFE_NO_PAD.encode(digest))
}

pub(crate) fn normalize_openai_responses_replay_ids(body: &mut Value) -> Result<(), ProviderError> {
    let Some(items) = body
        .as_object_mut()
        .and_then(|body| body.get_mut("input"))
        .and_then(Value::as_array_mut)
    else {
        return Ok(());
    };

    for item in items {
        let Some(item) = item.as_object_mut() else {
            continue;
        };
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => {
                normalize_optional_responses_item_id(item, "fc_")?;
                normalize_responses_call_id(item)?;
            }
            Some("function_call_output") => normalize_responses_call_id(item)?,
            Some("reasoning") => normalize_optional_responses_item_id(item, "rs_")?,
            Some("message") => normalize_optional_responses_item_id(item, "msg_")?,
            _ => {}
        }
    }
    Ok(())
}

fn normalize_optional_responses_item_id(
    item: &mut Map<String, Value>,
    native_prefix: &str,
) -> Result<(), ProviderError> {
    let Some(id) = item.get("id") else {
        return Ok(());
    };
    let id = id.as_str().ok_or_else(|| {
        ProviderError::InvalidRequest("OpenAI Responses item `id` must be a string".to_string())
    })?;
    if is_valid_responses_id(id, Some(native_prefix)) {
        return Ok(());
    }
    item.insert(
        "id".to_string(),
        Value::String(stable_prefixed_hash(native_prefix, id)),
    );
    Ok(())
}

fn normalize_responses_call_id(item: &mut Map<String, Value>) -> Result<(), ProviderError> {
    let Some(call_id) = item.get("call_id") else {
        return Ok(());
    };
    let call_id = call_id.as_str().ok_or_else(|| {
        ProviderError::InvalidRequest(
            "OpenAI Responses function call `call_id` must be a string".to_string(),
        )
    })?;
    if is_valid_responses_id(call_id, None) {
        return Ok(());
    }
    item.insert(
        "call_id".to_string(),
        Value::String(stable_prefixed_hash("call_", call_id)),
    );
    Ok(())
}

fn is_valid_responses_id(id: &str, required_prefix: Option<&str>) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && required_prefix.is_none_or(|prefix| id.starts_with(prefix) && id.len() > prefix.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
