use std::collections::BTreeMap;

use serde_json::{Map, Value};

pub(super) fn project_text(blocks: &[Value]) -> Option<String> {
    let mut text = String::new();
    for block in blocks {
        if matches!(
            block.get("type").and_then(Value::as_str),
            Some("text" | "input_text")
        ) {
            text.push_str(block.get("text")?.as_str()?);
        }
    }
    Some(text)
}

/// Assemble one native message while preserving block indices and all opaque start fields.
#[derive(Default)]
pub(crate) struct AnthropicReplayStream {
    blocks: BTreeMap<u64, Value>,
    arguments: BTreeMap<u64, String>,
    open: BTreeMap<u64, ()>,
    message: Map<String, Value>,
}

impl AnthropicReplayStream {
    pub(crate) fn is_incomplete_error(error: &str) -> bool {
        matches!(
            error,
            "native tool arguments are invalid JSON"
                | "native message finished with incomplete content blocks"
                | "native message stopped with incomplete content blocks"
        )
    }

    pub(crate) fn observe(&mut self, payload: &Value) -> Result<(), &'static str> {
        match payload.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                self.message = payload
                    .get("message")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
            }
            Some("content_block_start") => {
                let index = payload
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or("native content block has no index")?;
                let block = payload
                    .get("content_block")
                    .filter(|block| block.is_object())
                    .ok_or("native content block is not an object")?;
                if self.blocks.contains_key(&index) {
                    return Err("native content block index repeated");
                }
                self.blocks.insert(index, block.clone());
                self.open.insert(index, ());
            }
            Some("content_block_delta") => {
                let index = payload
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or("native content delta has no index")?;
                if !self.open.contains_key(&index) {
                    return Err("native content delta has no open block");
                }
                let block = self
                    .blocks
                    .get_mut(&index)
                    .ok_or("native content delta has no block")?;
                let delta = &payload["delta"];
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta" | "thinking_delta" | "signature_delta") => {
                        let field = match delta["type"].as_str() {
                            Some("text_delta") => "text",
                            Some("thinking_delta") => "thinking",
                            _ => "signature",
                        };
                        let part = delta
                            .get(field)
                            .and_then(Value::as_str)
                            .ok_or("native string delta is not a string")?;
                        let object = block
                            .as_object_mut()
                            .ok_or("native block is not an object")?;
                        let text = object
                            .entry(field)
                            .or_insert_with(|| Value::String(String::new()));
                        let Value::String(text) = text else {
                            return Err("native string delta does not match block");
                        };
                        text.push_str(part);
                    }
                    Some("input_json_delta") => {
                        let part = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .ok_or("native input delta is not a string")?;
                        self.arguments.entry(index).or_default().push_str(part);
                    }
                    Some("citations_delta") => {
                        let citation = delta
                            .get("citation")
                            .ok_or("native citation delta has no citation")?;
                        let object = block
                            .as_object_mut()
                            .ok_or("native block is not an object")?;
                        let citations = object
                            .entry("citations")
                            .or_insert_with(|| Value::Array(Vec::new()));
                        citations
                            .as_array_mut()
                            .ok_or("native citations are not an array")?
                            .push(citation.clone());
                    }
                    Some("compaction_delta") => {
                        if block.get("type").and_then(Value::as_str) != Some("compaction") {
                            return Err("native compaction delta does not match block");
                        }
                        let content = delta
                            .get("content")
                            .filter(|value| value.is_null() || value.is_string())
                            .ok_or("native compaction delta has invalid content")?;
                        let encrypted = delta.get("encrypted_content");
                        if encrypted.is_some_and(|value| !value.is_null() && !value.is_string()) {
                            return Err("native compaction delta has invalid encrypted content");
                        }
                        let object = block
                            .as_object_mut()
                            .ok_or("native block is not an object")?;
                        object.insert("content".into(), content.clone());
                        if let Some(encrypted) = encrypted {
                            object.insert("encrypted_content".into(), encrypted.clone());
                        }
                    }
                    _ => {
                        return Err(
                            "unsupported native content delta cannot be replayed faithfully",
                        );
                    }
                }
            }
            Some("content_block_stop") => {
                let index = payload
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or("native content stop has no index")?;
                if self.open.remove(&index).is_none() {
                    return Err("native content stop has no open block");
                }
                if let Some(arguments) = self.arguments.remove(&index) {
                    let input: Value = serde_json::from_str(&arguments)
                        .map_err(|_| "native tool arguments are invalid JSON")?;
                    self.blocks
                        .get_mut(&index)
                        .ok_or("native tool block is missing")?["input"] = input;
                }
            }
            Some("message_delta") => {
                if let Some(delta) = payload.get("delta").and_then(Value::as_object) {
                    if delta
                        .get("stop_reason")
                        .is_some_and(|reason| !reason.is_null())
                        && !self.open.is_empty()
                    {
                        return Err("native message finished with incomplete content blocks");
                    }
                    self.message.extend(delta.clone());
                }
            }
            Some("message_stop") if !self.open.is_empty() => {
                return Err("native message stopped with incomplete content blocks");
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn take_message(&mut self) -> Value {
        let mut message = std::mem::take(&mut self.message);
        message.insert(
            "content".into(),
            Value::Array(std::mem::take(&mut self.blocks).into_values().collect()),
        );
        Value::Object(message)
    }
}
