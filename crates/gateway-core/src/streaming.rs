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
    // Byte offset, deliberately not a string slice boundary. At most three old
    // bytes need another scan when a four-byte CRLF delimiter spans chunks.
    scan_offset: usize,
}

impl SseEventParser {
    pub fn push_bytes(&mut self, chunk: &[u8]) -> Result<Vec<ParsedSseEvent>, ProviderError> {
        let text = self.utf8.push_bytes(chunk)?;
        self.buffer.push_str(&text);

        let mut events = Vec::new();
        let mut consumed = 0;
        while let Some((delimiter_index, delimiter_len)) =
            find_sse_delimiter(self.buffer.as_bytes(), self.scan_offset)
        {
            let block = self.buffer[consumed..delimiter_index]
                .replace("\r\n", "\n")
                .replace('\r', "\n");
            consumed = delimiter_index + delimiter_len;
            self.scan_offset = consumed;

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

        self.scan_offset = self.buffer.len().saturating_sub(3).max(consumed) - consumed;
        // Compact once per input chunk, not once per event. A large chunk with
        // many small events must not repeatedly move the unconsumed suffix.
        if consumed > 0 {
            self.buffer.drain(..consumed);
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

fn find_sse_delimiter(input: &[u8], from: usize) -> Option<(usize, usize)> {
    for index in from..input.len().saturating_sub(1) {
        let suffix = &input[index..];
        if suffix.starts_with(b"\r\n\r\n") {
            return Some((index, 4));
        }
        if suffix.starts_with(b"\n\n") || suffix.starts_with(b"\r\r") {
            return Some((index, 2));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{SseEventParser, Utf8ChunkDecoder};

    #[test]
    fn sse_events_survive_every_chunk_boundary_and_single_byte_chunks() {
        for delimiter in ["\n\n", "\r\r", "\r\n\r\n"] {
            let input = format!("event: update\ndata: 🙂 café{delimiter}data: next{delimiter}");
            for split in 0..=input.len() {
                let mut parser = SseEventParser::default();
                let mut events = parser.push_bytes(&input.as_bytes()[..split]).unwrap();
                events.extend(parser.push_bytes(&input.as_bytes()[split..]).unwrap());
                assert_eq!(events.len(), 2, "split {split}, delimiter {delimiter:?}");
                assert_eq!(events[0].event.as_deref(), Some("update"));
                assert_eq!(events[0].data, "🙂 café");
                assert_eq!(events[1].data, "next");
                parser.finish().unwrap();
            }
            let mut parser = SseEventParser::default();
            let mut events = Vec::new();
            for byte in input.as_bytes() {
                events.extend(parser.push_bytes(&[*byte]).unwrap());
            }
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].data, "🙂 café");
            assert_eq!(events[1].data, "next");
            parser.finish().unwrap();
        }
    }

    #[test]
    fn sse_scan_retains_only_delimiter_overlap_for_large_incomplete_events() {
        let mut parser = SseEventParser::default();
        let chunk = "🙂".repeat(1024);
        parser.push_bytes(b"data: ").unwrap();
        for _ in 0..256 {
            assert!(parser.push_bytes(chunk.as_bytes()).unwrap().is_empty());
            assert_eq!(parser.scan_offset, parser.buffer.len() - 3);
        }
        let events = parser.push_bytes(b"\n\n").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, chunk.repeat(256));
        assert!(parser.buffer.is_empty());
        assert_eq!(parser.scan_offset, 0);
        parser.finish().unwrap();
    }

    #[test]
    fn sse_compaction_preserves_partial_suffix_after_many_events() {
        let mut parser = SseEventParser::default();
        let input = format!("{}data: partial", "data: complete\n\n".repeat(10_000));
        let events = parser.push_bytes(input.as_bytes()).unwrap();
        assert_eq!(events.len(), 10_000);
        assert!(events.iter().all(|event| event.data == "complete"));
        assert!(parser.finish().is_err());
        let events = parser.push_bytes(b" suffix\r\n\r\n").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "partial suffix");
        parser.finish().unwrap();
    }

    #[test]
    fn sse_parser_rejects_invalid_or_truncated_utf8() {
        let mut parser = SseEventParser::default();
        assert!(parser.push_bytes(b"data: \xff").is_err());
        let mut parser = SseEventParser::default();
        parser.push_bytes(b"data: \xf0\x9f").unwrap();
        assert!(parser.finish().is_err());
    }

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
