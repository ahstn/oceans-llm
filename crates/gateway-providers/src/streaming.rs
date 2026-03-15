use bytes::Bytes;
use gateway_core::ProviderError;
use serde_json::json;

#[derive(Debug, Clone)]
pub(crate) struct ParsedSseEvent {
    pub(crate) event: Option<String>,
    pub(crate) data: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SseEventParser {
    utf8: Utf8ChunkDecoder,
    buffer: String,
}

impl SseEventParser {
    pub(crate) fn push_bytes(
        &mut self,
        chunk: &[u8],
    ) -> Result<Vec<ParsedSseEvent>, ProviderError> {
        let text = self.utf8.push_bytes(chunk)?;
        self.buffer.push_str(&text);

        let mut events = Vec::new();
        while let Some((delimiter_index, delimiter_len)) = find_sse_delimiter(&self.buffer) {
            let block = self.buffer[..delimiter_index]
                .replace("\r\n", "\n")
                .replace('\r', "\n");
            self.buffer.drain(..delimiter_index + delimiter_len);

            let mut event_type = None;
            let mut data_lines = Vec::new();
            for line in block.lines() {
                if let Some(rest) = line.strip_prefix("event:") {
                    event_type = Some(rest.trim().to_string());
                } else if let Some(rest) = line.strip_prefix("data:") {
                    data_lines.push(rest.trim_start().to_string());
                }
            }

            events.push(ParsedSseEvent {
                event: event_type,
                data: data_lines.join("\n"),
            });
        }

        Ok(events)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Utf8ChunkDecoder {
    pending: Vec<u8>,
}

impl Utf8ChunkDecoder {
    pub(crate) fn push_bytes(&mut self, chunk: &[u8]) -> Result<String, ProviderError> {
        if self.pending.is_empty() {
            match std::str::from_utf8(chunk) {
                Ok(text) => return Ok(text.to_string()),
                Err(error) if error.error_len().is_some() => {
                    return Err(ProviderError::Transport(format!(
                        "stream chunk was not utf8: {error}"
                    )));
                }
                Err(_) => {}
            }
        }

        self.pending.extend_from_slice(chunk);
        match std::str::from_utf8(&self.pending) {
            Ok(text) => {
                let owned = text.to_string();
                self.pending.clear();
                Ok(owned)
            }
            Err(error) if error.error_len().is_some() => Err(ProviderError::Transport(format!(
                "stream chunk was not utf8: {error}"
            ))),
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                if valid_up_to == 0 {
                    return Ok(String::new());
                }

                let valid = std::str::from_utf8(&self.pending[..valid_up_to]).map_err(|error| {
                    ProviderError::Transport(format!("stream chunk was not utf8: {error}"))
                })?;
                let owned = valid.to_string();
                self.pending.drain(..valid_up_to);
                Ok(owned)
            }
        }
    }
}

pub(crate) fn openai_sse_error_chunk(kind: &str, message: &str) -> Bytes {
    Bytes::from(format!(
        "data: {}\n\n",
        json!({
            "error": {
                "message": message,
                "type": "upstream_error",
                "code": kind
            }
        })
    ))
}

pub(crate) fn done_sse_chunk() -> Bytes {
    Bytes::from("data: [DONE]\n\n")
}

pub(crate) fn render_sse_event_chunk(event: Option<&str>, data: &str) -> Bytes {
    let mut rendered = String::new();
    if let Some(event) = event {
        rendered.push_str("event: ");
        rendered.push_str(event);
        rendered.push('\n');
    }

    for line in data.split('\n') {
        rendered.push_str("data: ");
        rendered.push_str(line);
        rendered.push('\n');
    }
    rendered.push('\n');

    Bytes::from(rendered)
}

fn find_sse_delimiter(input: &str) -> Option<(usize, usize)> {
    [
        input.find("\r\n\r\n").map(|index| (index, 4)),
        input.find("\n\n").map(|index| (index, 2)),
        input.find("\r\r").map(|index| (index, 2)),
    ]
    .into_iter()
    .flatten()
    .min_by_key(|(index, _)| *index)
}
