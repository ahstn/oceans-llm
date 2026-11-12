use std::collections::BTreeMap;

use bytes::{Buf, Bytes};
use gateway_core::ProviderError;

#[derive(Debug, Clone)]
pub(super) struct BedrockEvent {
    pub(super) message_type: Option<String>,
    pub(super) event_type: Option<String>,
    pub(super) exception_type: Option<String>,
    pub(super) payload: Bytes,
}

#[derive(Debug)]
pub(super) struct BedrockDecodedBatch {
    pub(super) events: Vec<BedrockEvent>,
    pub(super) error: Option<ProviderError>,
}

#[derive(Debug, Default)]
pub(super) struct BedrockEventStreamDecoder {
    buffer: Vec<u8>,
    failed: bool,
}

impl BedrockEventStreamDecoder {
    const PRELUDE_LEN: usize = 12;
    const MESSAGE_CRC_LEN: usize = 4;
    const MAX_FRAME_LEN: usize = 16 * 1024 * 1024;

    pub(super) fn push_bytes(
        &mut self,
        chunk: &[u8],
    ) -> Result<BedrockDecodedBatch, ProviderError> {
        if self.failed {
            return Err(ProviderError::Transport(
                "aws_bedrock EventStream decoder already failed".to_string(),
            ));
        }
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();

        loop {
            match self.decode_frame() {
                Ok(Some(event)) => events.push(event),
                Ok(None) => break,
                Err(error) => {
                    self.failed = true;
                    if events.is_empty() {
                        return Err(error);
                    }
                    return Ok(BedrockDecodedBatch {
                        events,
                        error: Some(error),
                    });
                }
            }
        }
        Ok(BedrockDecodedBatch {
            events,
            error: None,
        })
    }

    fn decode_frame(&mut self) -> Result<Option<BedrockEvent>, ProviderError> {
        if self.buffer.len() < Self::PRELUDE_LEN {
            return Ok(None);
        }
        let total_len = u32::from_be_bytes(self.buffer[0..4].try_into().unwrap()) as usize;
        let headers_len = u32::from_be_bytes(self.buffer[4..8].try_into().unwrap()) as usize;
        if total_len < Self::PRELUDE_LEN + Self::MESSAGE_CRC_LEN {
            return Err(ProviderError::Transport(format!(
                "invalid aws_bedrock EventStream frame length `{total_len}`"
            )));
        }
        if total_len > Self::MAX_FRAME_LEN {
            return Err(ProviderError::Transport(format!(
                "aws_bedrock EventStream frame length `{total_len}` exceeds limit"
            )));
        }
        let payload_start = Self::PRELUDE_LEN.checked_add(headers_len).ok_or_else(|| {
            ProviderError::Transport("aws_bedrock EventStream headers length overflow".to_string())
        })?;
        let payload_end = total_len
            .checked_sub(Self::MESSAGE_CRC_LEN)
            .ok_or_else(|| {
                ProviderError::Transport(
                    "aws_bedrock EventStream payload length underflow".to_string(),
                )
            })?;
        if payload_start > payload_end {
            return Err(ProviderError::Transport(format!(
                "aws_bedrock EventStream headers length `{headers_len}` exceeds frame payload"
            )));
        }
        let prelude_crc = u32::from_be_bytes(self.buffer[8..12].try_into().unwrap());
        if crc32fast::hash(&self.buffer[..8]) != prelude_crc {
            return Err(ProviderError::Transport(
                "aws_bedrock EventStream prelude checksum mismatch".to_string(),
            ));
        }
        if self.buffer.len() < total_len {
            return Ok(None);
        }
        let message_crc =
            u32::from_be_bytes(self.buffer[payload_end..total_len].try_into().unwrap());
        if crc32fast::hash(&self.buffer[..payload_end]) != message_crc {
            return Err(ProviderError::Transport(
                "aws_bedrock EventStream message checksum mismatch".to_string(),
            ));
        }
        let frame = self.buffer.drain(..total_len).collect::<Vec<_>>();
        let headers = parse_eventstream_headers(&frame[Self::PRELUDE_LEN..payload_start])?;
        let payload = Bytes::copy_from_slice(&frame[payload_start..payload_end]);
        Ok(Some(BedrockEvent {
            message_type: headers.get(":message-type").cloned(),
            event_type: headers.get(":event-type").cloned(),
            exception_type: headers.get(":exception-type").cloned(),
            payload,
        }))
    }

    pub(super) fn finish(&mut self) -> Result<(), ProviderError> {
        if self.failed {
            Err(ProviderError::Transport(
                "aws_bedrock EventStream decoder already failed".to_string(),
            ))
        } else if self.buffer.is_empty() {
            Ok(())
        } else {
            Err(ProviderError::Transport(format!(
                "stream ended with incomplete aws_bedrock EventStream frame ({} bytes buffered)",
                self.buffer.len()
            )))
        }
    }
}

pub(super) fn parse_eventstream_headers(
    headers: &[u8],
) -> Result<BTreeMap<String, String>, ProviderError> {
    let mut cursor = headers;
    let mut parsed = BTreeMap::new();

    while cursor.has_remaining() {
        if cursor.remaining() < 1 {
            return Err(ProviderError::Transport(
                "malformed aws_bedrock EventStream header name length".to_string(),
            ));
        }
        let name_len = cursor.get_u8() as usize;
        if cursor.remaining() < name_len + 1 {
            return Err(ProviderError::Transport(
                "malformed aws_bedrock EventStream header".to_string(),
            ));
        }
        let name = std::str::from_utf8(&cursor[..name_len]).map_err(|error| {
            ProviderError::Transport(format!(
                "aws_bedrock EventStream header name was not utf8: {error}"
            ))
        })?;
        cursor.advance(name_len);
        let value_type = cursor.get_u8();

        let value = match value_type {
            7 => {
                if cursor.remaining() < 2 {
                    return Err(ProviderError::Transport(
                        "malformed aws_bedrock EventStream string header length".to_string(),
                    ));
                }
                let value_len = cursor.get_u16() as usize;
                if cursor.remaining() < value_len {
                    return Err(ProviderError::Transport(
                        "malformed aws_bedrock EventStream string header value".to_string(),
                    ));
                }
                let value = std::str::from_utf8(&cursor[..value_len]).map_err(|error| {
                    ProviderError::Transport(format!(
                        "aws_bedrock EventStream string header was not utf8: {error}"
                    ))
                })?;
                cursor.advance(value_len);
                value.to_string()
            }
            0 => "true".to_string(),
            1 => "false".to_string(),
            2..=6 | 8 | 9 => {
                let value_len = match value_type {
                    2 => 1,
                    3 => 2,
                    4 => 4,
                    5 | 8 => 8,
                    9 => 16,
                    6 => {
                        if cursor.remaining() < 2 {
                            return Err(ProviderError::Transport(
                                "malformed aws_bedrock EventStream byte header length".to_string(),
                            ));
                        }
                        cursor.get_u16() as usize
                    }
                    _ => unreachable!(),
                };
                if cursor.remaining() < value_len {
                    return Err(ProviderError::Transport(
                        "malformed aws_bedrock EventStream typed header value".to_string(),
                    ));
                }
                cursor.advance(value_len);
                continue;
            }
            other => {
                return Err(ProviderError::Transport(format!(
                    "unsupported aws_bedrock EventStream header value type `{other}`"
                )));
            }
        };
        parsed.insert(name.to_string(), value);
    }

    Ok(parsed)
}
