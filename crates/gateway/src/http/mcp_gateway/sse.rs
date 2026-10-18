//! Server-sent event sanitising for upstream MCP streams.
//!
//! The gateway does not support stream resumption: it never forwards
//! `Last-Event-ID`, and it removes the `id` field from every event so clients
//! have nothing to resume from. An upstream replay would skip the grant
//! filtering, guardrails and invocation logging that the original request got.
//!
//! On the standalone `GET` stream the server may only send requests and
//! notifications. JSON-RPC responses are dropped there, so an upstream cannot
//! deliver a tool result outside the `tools/call` that the gateway governs.

use std::io;

use axum::body::Bytes;
use futures_util::{Stream, StreamExt, stream};
use serde_json::Value;

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
/// Most bytes one unfinished event may hold. An upstream that never ends an
/// event would otherwise grow the buffer for the life of the stream.
pub(super) const MAX_SSE_EVENT_BYTES: usize = super::MAX_MCP_REWRITE_BODY_BYTES as usize;

/// Incremental SSE rewriter. Feed it upstream chunks in order and call
/// [`SseSanitizer::finish`] once the stream ends.
#[derive(Debug)]
pub(super) struct SseSanitizer {
    /// Normalised bytes not yet emitted: line endings are `\n` only.
    pending: Vec<u8>,
    /// Bytes of `pending` already searched for an event end, so a large event
    /// arriving in many chunks is not rescanned from its start each time.
    scanned: usize,
    /// A trailing `\r` that may be the first half of `\r\n`.
    carried_cr: bool,
    /// Set until the first bytes show whether the stream opens with a BOM.
    at_start: bool,
    /// The first bytes, held while they could still be a split BOM.
    head: Vec<u8>,
    drop_responses: bool,
}

impl SseSanitizer {
    /// `drop_responses` is set for the standalone `GET` stream.
    pub(super) fn new(drop_responses: bool) -> Self {
        Self {
            pending: Vec::new(),
            scanned: 0,
            carried_cr: false,
            at_start: true,
            head: Vec::new(),
            drop_responses,
        }
    }

    /// Accepts the next upstream chunk and returns the complete events it
    /// finished, sanitised. Fails once an unfinished event passes
    /// [`MAX_SSE_EVENT_BYTES`].
    pub(super) fn push(&mut self, chunk: &[u8]) -> Result<Bytes, io::Error> {
        if self.at_start {
            self.head.extend_from_slice(chunk);
            // A BOM split across chunks must be stripped too, or the first
            // field would no longer be recognised.
            if self.head.len() < UTF8_BOM.len() && UTF8_BOM.starts_with(&self.head) {
                return Ok(Bytes::new());
            }
            self.at_start = false;
            let head = std::mem::take(&mut self.head);
            self.normalise_into_pending(head.strip_prefix(UTF8_BOM).unwrap_or(&head));
        } else {
            self.normalise_into_pending(chunk);
        }
        let mut out = Vec::new();
        // The last byte already searched may be the first half of `\n\n`.
        let mut from = self.scanned.saturating_sub(1);
        while let Some(end) = find_event_end(&self.pending[from..]).map(|end| from + end) {
            let event: Vec<u8> = self.pending.drain(..end + 2).collect();
            self.write_event(&event[..end], &mut out);
            from = 0;
        }
        self.scanned = self.pending.len();
        if self.pending.len() > MAX_SSE_EVENT_BYTES {
            return Err(io::Error::other(format!(
                "upstream SSE event exceeds {MAX_SSE_EVENT_BYTES} bytes"
            )));
        }
        Ok(Bytes::from(out))
    }

    /// Emits whatever remains when the upstream stream ends.
    pub(super) fn finish(&mut self) -> Bytes {
        if std::mem::take(&mut self.at_start) {
            // Fewer bytes than a BOM, so they are not one.
            let head = std::mem::take(&mut self.head);
            self.normalise_into_pending(&head);
        }
        if std::mem::take(&mut self.carried_cr) {
            self.pending.push(b'\n');
        }
        let rest = std::mem::take(&mut self.pending);
        self.scanned = 0;
        let mut out = Vec::new();
        let trimmed = rest.strip_suffix(b"\n").unwrap_or(&rest);
        if !trimmed.is_empty() {
            self.write_event(trimmed, &mut out);
        }
        Bytes::from(out)
    }

    fn normalise_into_pending(&mut self, chunk: &[u8]) {
        let mut bytes = chunk.iter().copied().peekable();
        if std::mem::take(&mut self.carried_cr) {
            self.pending.push(b'\n');
            if bytes.peek() == Some(&b'\n') {
                bytes.next();
            }
        }
        while let Some(byte) = bytes.next() {
            match byte {
                b'\r' => match bytes.peek() {
                    Some(b'\n') => {
                        bytes.next();
                        self.pending.push(b'\n');
                    }
                    Some(_) => self.pending.push(b'\n'),
                    None => self.carried_cr = true,
                },
                other => self.pending.push(other),
            }
        }
    }

    /// Writes one event (without its terminating blank line), minus its `id`
    /// field. Events that are only an `id` are dropped entirely.
    fn write_event(&self, event: &[u8], out: &mut Vec<u8>) {
        let lines = event
            .split(|byte| *byte == b'\n')
            .filter(|line| !is_id_field(line))
            .collect::<Vec<_>>();
        if lines.iter().all(|line| line.is_empty()) {
            return;
        }
        if self.drop_responses && is_json_rpc_response(&lines) {
            return;
        }
        for line in lines {
            out.extend_from_slice(line);
            out.push(b'\n');
        }
        out.push(b'\n');
    }
}

fn find_event_end(pending: &[u8]) -> Option<usize> {
    pending.windows(2).position(|pair| pair == b"\n\n")
}

/// An SSE line whose field name is `id`, with or without a value.
fn is_id_field(line: &[u8]) -> bool {
    line == b"id" || line.starts_with(b"id:")
}

/// Whether the event's data is a JSON-RPC response (a result or error for
/// some request id) rather than a server request or notification.
fn is_json_rpc_response(lines: &[&[u8]]) -> bool {
    let data = lines
        .iter()
        .filter_map(|line| line.strip_prefix(b"data:"))
        .map(|data| data.strip_prefix(b" ").unwrap_or(data))
        .collect::<Vec<_>>()
        .join(&b'\n');
    if data.is_empty() {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<Value>(&data) else {
        // Not JSON-RPC at all; a client cannot treat it as a response either.
        return false;
    };
    let is_response = |message: &Value| {
        message.get("method").is_none()
            && (message.get("result").is_some() || message.get("error").is_some())
    };
    match &value {
        Value::Array(batch) => batch.iter().any(is_response),
        message => is_response(message),
    }
}

/// Sanitises a complete SSE body.
pub(super) fn sanitize_sse_body(body: &[u8], drop_responses: bool) -> Result<Bytes, io::Error> {
    let mut sanitizer = SseSanitizer::new(drop_responses);
    let mut out = sanitizer.push(body)?.to_vec();
    out.extend_from_slice(&sanitizer.finish());
    Ok(Bytes::from(out))
}

/// Sanitises an SSE byte stream as it arrives.
pub(super) fn sanitize_sse_stream<S>(
    upstream: S,
    drop_responses: bool,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static
where
    S: Stream<Item = Result<Bytes, io::Error>> + Send + Unpin + 'static,
{
    let state = (upstream, SseSanitizer::new(drop_responses), false);
    stream::unfold(state, |(mut upstream, mut sanitizer, done)| async move {
        if done {
            return None;
        }
        loop {
            match upstream.next().await {
                Some(Ok(chunk)) => match sanitizer.push(&chunk) {
                    Ok(out) if out.is_empty() => {}
                    Ok(out) => return Some((Ok(out), (upstream, sanitizer, false))),
                    Err(error) => return Some((Err(error), (upstream, sanitizer, true))),
                },
                Some(Err(error)) => return Some((Err(error), (upstream, sanitizer, true))),
                None => {
                    let out = sanitizer.finish();
                    return Some((Ok(out), (upstream, sanitizer, true)));
                }
            }
        }
    })
    .filter(|item| std::future::ready(!matches!(item, Ok(bytes) if bytes.is_empty())))
}

#[cfg(test)]
mod tests {
    use futures_util::{StreamExt, stream};

    use super::*;

    fn sanitize(body: &str, drop_responses: bool) -> String {
        String::from_utf8(
            sanitize_sse_body(body.as_bytes(), drop_responses)
                .unwrap()
                .to_vec(),
        )
        .unwrap()
    }

    #[test]
    fn event_ids_are_removed_and_other_fields_kept() {
        let body = "id: 7\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"n\"}\n\nid\n\n";
        assert_eq!(
            sanitize(body, false),
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"n\"}\n\n"
        );
    }

    #[test]
    fn get_streams_drop_json_rpc_responses() {
        let body = concat!(
            "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"content\":[]}}\n\n",
            "data: {\"jsonrpc\":\"2.0\",\"id\":2,\"error\":{\"code\":1,\"message\":\"x\"}}\n\n",
            "data: {\"jsonrpc\":\"2.0\",\"id\":\"s1\",\"method\":\"roots/list\"}\n\n",
        );
        assert_eq!(
            sanitize(body, true),
            "data: {\"jsonrpc\":\"2.0\",\"id\":\"s1\",\"method\":\"roots/list\"}\n\n"
        );
        assert_eq!(sanitize(body, false), body);
    }

    #[test]
    fn line_endings_and_a_bom_are_normalised() {
        let body = "\u{feff}id: 1\r\ndata: {}\r\rid: 2\rdata: [1]\r\n\r\n";
        assert_eq!(sanitize(body, false), "data: {}\n\ndata: [1]\n\n");
    }

    #[tokio::test]
    async fn chunk_boundaries_inside_crlf_and_events_are_handled() {
        let chunks = [
            "id: 1\r",
            "\ndata: {\"a\"",
            ":1}\r",
            "\n\r",
            "\nid: 2\ndata: {}",
        ];
        let upstream = stream::iter(
            chunks
                .into_iter()
                .map(|chunk| Ok::<_, io::Error>(Bytes::from(chunk))),
        );
        let out = sanitize_sse_stream(upstream, false)
            .map(|chunk| chunk.unwrap())
            .collect::<Vec<_>>()
            .await
            .concat();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "data: {\"a\":1}\n\ndata: {}\n\n"
        );
    }

    #[tokio::test]
    async fn a_bom_split_across_chunks_is_still_stripped() {
        let chunks: [&[u8]; 3] = [
            b"\xEF",
            b"\xBB",
            b"\xBFid: 1\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n",
        ];
        let upstream = stream::iter(
            chunks
                .into_iter()
                .map(|chunk| Ok::<_, io::Error>(Bytes::from_static(chunk))),
        );
        let out = sanitize_sse_stream(upstream, true)
            .map(|chunk| chunk.unwrap())
            .collect::<Vec<_>>()
            .await
            .concat();
        assert!(out.is_empty(), "{:?}", String::from_utf8_lossy(&out));
    }

    #[tokio::test]
    async fn an_event_that_never_ends_stops_the_stream() {
        let line = Bytes::from(vec![b'x'; 1024 * 1024]);
        let upstream = stream::iter(
            std::iter::once(Ok::<_, io::Error>(Bytes::from_static(b"data: ")))
                .chain(std::iter::repeat_n(line, 8).map(Ok)),
        );
        let items = sanitize_sse_stream(upstream, false)
            .collect::<Vec<_>>()
            .await;
        assert!(matches!(items.last(), Some(Err(_))));
        assert_eq!(
            items.len(),
            1,
            "nothing of the oversized event was forwarded"
        );
    }

    #[test]
    fn an_event_end_split_across_chunks_is_found_after_the_resumed_search() {
        let mut sanitizer = SseSanitizer::new(false);
        assert!(sanitizer.push(b"data: {\"a\":1}").unwrap().is_empty());
        assert!(sanitizer.push(b"\n").unwrap().is_empty());
        assert_eq!(
            sanitizer.push(b"\ndata: {}").unwrap(),
            Bytes::from_static(b"data: {\"a\":1}\n\n")
        );
        assert_eq!(sanitizer.finish(), Bytes::from_static(b"data: {}\n\n"));
    }
}
