use crate::ProviderError;

#[derive(Debug, Clone)]
pub struct ParsedSseEvent {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Debug, Clone, Default)]
pub struct SseEventParser {
    utf8: Utf8ChunkDecoder,
    buffer: String,
}

impl SseEventParser {
    pub fn push_bytes(&mut self, chunk: &[u8]) -> Result<Vec<ParsedSseEvent>, ProviderError> {
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

    pub fn finish(&mut self) -> Result<(), ProviderError> {
        self.utf8.finish()?;
        if !self.buffer.trim().is_empty() {
            return Err(ProviderError::Transport(
                "stream ended with an incomplete sse event".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct Utf8ChunkDecoder {
    pending: Vec<u8>,
}

impl Utf8ChunkDecoder {
    pub fn push_bytes(&mut self, chunk: &[u8]) -> Result<String, ProviderError> {
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

    pub fn finish(&self) -> Result<(), ProviderError> {
        if !self.pending.is_empty() {
            return Err(ProviderError::Transport(
                "stream ended with incomplete utf8 bytes".to_string(),
            ));
        }
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::{SseEventParser, Utf8ChunkDecoder};

    #[test]
    fn utf8_decoder_reassembles_split_codepoints() {
        let mut decoder = Utf8ChunkDecoder::default();

        assert_eq!(decoder.push_bytes(&[0xF0, 0x9F]).expect("first"), "");
        assert_eq!(
            decoder.push_bytes(&[0x99, 0x82]).expect("second"),
            "\u{1F642}"
        );
        decoder.finish().expect("finish");
    }

    #[test]
    fn sse_parser_reassembles_split_lines_and_supports_colon_without_space() {
        let mut parser = SseEventParser::default();

        assert!(
            parser
                .push_bytes(b"event: message\ndata:{\"a\"")
                .expect("part1")
                .is_empty()
        );
        let events = parser
            .push_bytes(b":1}\ndata: {\"b\":2}\n\n")
            .expect("part2");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.as_deref(), Some("message"));
        assert_eq!(events[0].data, "{\"a\":1}\n{\"b\":2}");
        parser.finish().expect("finish");
    }

    #[test]
    fn sse_parser_reassembles_crlf_delimited_events() {
        let mut parser = SseEventParser::default();

        assert!(
            parser
                .push_bytes(b"data: {\"value\":1}\r\n")
                .expect("part1")
                .is_empty()
        );
        let events = parser.push_bytes(b"\r\n").expect("part2");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "{\"value\":1}");
        parser.finish().expect("finish");
    }
}
