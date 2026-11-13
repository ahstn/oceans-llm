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

#[derive(Debug, Clone, Copy)]
pub(super) enum ReplayError {
    MissingBlockIndex,
    InvalidBlock,
    RepeatedBlockIndex,
    MissingDeltaIndex,
    DeltaWithoutOpenBlock,
    DeltaWithoutBlock,
    InvalidStringDelta,
    InvalidBlockObject,
    MismatchedStringDelta,
    InvalidInputDelta,
    MissingCitation,
    InvalidCitations,
    MismatchedCompactionDelta,
    InvalidCompactionContent,
    InvalidCompactionEncryptedContent,
    UnsupportedDelta,
    MissingStopIndex,
    StopWithoutOpenBlock,
    InvalidToolArguments,
    MissingToolBlock,
    IncompleteMessageFinish,
    IncompleteMessageStop,
}

impl ReplayError {
    pub(super) fn is_incomplete(self) -> bool {
        matches!(
            self,
            Self::InvalidToolArguments
                | Self::IncompleteMessageFinish
                | Self::IncompleteMessageStop
        )
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::MissingBlockIndex => "native content block has no index",
            Self::InvalidBlock => "native content block is not an object",
            Self::RepeatedBlockIndex => "native content block index repeated",
            Self::MissingDeltaIndex => "native content delta has no index",
            Self::DeltaWithoutOpenBlock => "native content delta has no open block",
            Self::DeltaWithoutBlock => "native content delta has no block",
            Self::InvalidStringDelta => "native string delta is not a string",
            Self::InvalidBlockObject => "native block is not an object",
            Self::MismatchedStringDelta => "native string delta does not match block",
            Self::InvalidInputDelta => "native input delta is not a string",
            Self::MissingCitation => "native citation delta has no citation",
            Self::InvalidCitations => "native citations are not an array",
            Self::MismatchedCompactionDelta => "native compaction delta does not match block",
            Self::InvalidCompactionContent => "native compaction delta has invalid content",
            Self::InvalidCompactionEncryptedContent => {
                "native compaction delta has invalid encrypted content"
            }
            Self::UnsupportedDelta => {
                "unsupported native content delta cannot be replayed faithfully"
            }
            Self::MissingStopIndex => "native content stop has no index",
            Self::StopWithoutOpenBlock => "native content stop has no open block",
            Self::InvalidToolArguments => "native tool arguments are invalid JSON",
            Self::MissingToolBlock => "native tool block is missing",
            Self::IncompleteMessageFinish => {
                "native message finished with incomplete content blocks"
            }
            Self::IncompleteMessageStop => "native message stopped with incomplete content blocks",
        }
    }
}

/// Assemble one native message while preserving block indices and all opaque start fields.
#[derive(Default)]
pub(super) struct AnthropicReplayStream {
    blocks: BTreeMap<u64, Value>,
    arguments: BTreeMap<u64, String>,
    open: BTreeMap<u64, ()>,
    message: Map<String, Value>,
}

impl AnthropicReplayStream {
    pub(super) fn observe(&mut self, payload: &Value) -> Result<(), ReplayError> {
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
                    .ok_or(ReplayError::MissingBlockIndex)?;
                let block = payload
                    .get("content_block")
                    .filter(|block| block.is_object())
                    .ok_or(ReplayError::InvalidBlock)?;
                if self.blocks.contains_key(&index) {
                    return Err(ReplayError::RepeatedBlockIndex);
                }
                self.blocks.insert(index, block.clone());
                self.open.insert(index, ());
            }
            Some("content_block_delta") => {
                let index = payload
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or(ReplayError::MissingDeltaIndex)?;
                if !self.open.contains_key(&index) {
                    return Err(ReplayError::DeltaWithoutOpenBlock);
                }
                let block = self
                    .blocks
                    .get_mut(&index)
                    .ok_or(ReplayError::DeltaWithoutBlock)?;
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
                            .ok_or(ReplayError::InvalidStringDelta)?;
                        let object = block
                            .as_object_mut()
                            .ok_or(ReplayError::InvalidBlockObject)?;
                        let text = object
                            .entry(field)
                            .or_insert_with(|| Value::String(String::new()));
                        let Value::String(text) = text else {
                            return Err(ReplayError::MismatchedStringDelta);
                        };
                        text.push_str(part);
                    }
                    Some("input_json_delta") => {
                        let part = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .ok_or(ReplayError::InvalidInputDelta)?;
                        self.arguments.entry(index).or_default().push_str(part);
                    }
                    Some("citations_delta") => {
                        let citation = delta.get("citation").ok_or(ReplayError::MissingCitation)?;
                        let object = block
                            .as_object_mut()
                            .ok_or(ReplayError::InvalidBlockObject)?;
                        let citations = object
                            .entry("citations")
                            .or_insert_with(|| Value::Array(Vec::new()));
                        citations
                            .as_array_mut()
                            .ok_or(ReplayError::InvalidCitations)?
                            .push(citation.clone());
                    }
                    Some("compaction_delta") => {
                        if block.get("type").and_then(Value::as_str) != Some("compaction") {
                            return Err(ReplayError::MismatchedCompactionDelta);
                        }
                        let content = delta
                            .get("content")
                            .filter(|value| value.is_null() || value.is_string())
                            .ok_or(ReplayError::InvalidCompactionContent)?;
                        let encrypted = delta.get("encrypted_content");
                        if encrypted.is_some_and(|value| !value.is_null() && !value.is_string()) {
                            return Err(ReplayError::InvalidCompactionEncryptedContent);
                        }
                        let object = block
                            .as_object_mut()
                            .ok_or(ReplayError::InvalidBlockObject)?;
                        object.insert("content".into(), content.clone());
                        if let Some(encrypted) = encrypted {
                            object.insert("encrypted_content".into(), encrypted.clone());
                        }
                    }
                    _ => {
                        return Err(ReplayError::UnsupportedDelta);
                    }
                }
            }
            Some("content_block_stop") => {
                let index = payload
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or(ReplayError::MissingStopIndex)?;
                if self.open.remove(&index).is_none() {
                    return Err(ReplayError::StopWithoutOpenBlock);
                }
                // A no-argument tool streams only an empty fragment; its start `input` stands.
                if let Some(arguments) = self.arguments.remove(&index)
                    && !arguments.is_empty()
                {
                    let input: Value = serde_json::from_str(&arguments)
                        .map_err(|_| ReplayError::InvalidToolArguments)?;
                    self.blocks
                        .get_mut(&index)
                        .ok_or(ReplayError::MissingToolBlock)?["input"] = input;
                }
            }
            Some("message_delta") => {
                if let Some(delta) = payload.get("delta").and_then(Value::as_object) {
                    if delta
                        .get("stop_reason")
                        .is_some_and(|reason| !reason.is_null())
                        && !self.open.is_empty()
                    {
                        return Err(ReplayError::IncompleteMessageFinish);
                    }
                    self.message.extend(delta.clone());
                }
            }
            Some("message_stop") if !self.open.is_empty() => {
                return Err(ReplayError::IncompleteMessageStop);
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn take_message(&mut self) -> Value {
        let mut message = std::mem::take(&mut self.message);
        message.insert(
            "content".into(),
            Value::Array(std::mem::take(&mut self.blocks).into_values().collect()),
        );
        Value::Object(message)
    }
}
