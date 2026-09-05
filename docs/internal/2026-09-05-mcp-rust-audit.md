# MCP Rust audit

Audit date: 2026-09-05. Scope: `crates/gateway-mcp/` and `crates/gateway/src/http/mcp_*`.

The Workbench UI and connection configuration work was committed as `13fe5a53` on `codex/mcp-workbench`. The branch then merged `origin/main` at `4788f399` through merge commit `c7d97b81`. The merge had no conflicts. The audit uses that merged source and preserves its recent authentication, OAuth transaction, and SSE boundary tests.

The audit found concrete transport, policy, and persistence defects. The changes below fix these defects and simplify the code that owns them.

| Area | Finding | Result |
| --- | --- | --- |
| Response correlation | JSON responses did not check the request ID. SSE responses decoded a typed result before checking its ID, so an unrelated result could abort the request. | Both transports check IDs before typed result decoding. Loopback HTTP tests cover mismatched IDs and unrelated SSE results. |
| Client transport | Initialization could send different protocol versions in the header and body. Body read timeouts were classified as generic transport failures. | Initialization uses the configured version consistently; body read timeouts retain the timeout category. |
| URL disclosure | Reqwest diagnostics could include configured upstream URLs and query credentials. Invalid URL errors also displayed the original endpoint. | Proxy and client transport errors remove the URL. Invalid URL display includes only the parser reason. A refused-connection test reproduced the leak before the fix. |
| SSE boundaries | BOM-prefixed tool lists could bypass filtering. CR-only or mixed line endings could bypass direct result evaluation. | The parsers handle an initial UTF-8 BOM and CR, LF, or CRLF line endings. Tests verify tool filtering and result transformations, including preserved response IDs and SSE fields. |
| Ambiguous envelopes | Mixed `result` and `error` fields could leave content unfiltered or unguarded. Client responses with missing or invalid envelope fields were accepted. | Tool lists filter any result present; guarded results reject mixed envelopes. Client response parsing checks version, ID shape, and result/error presence. |
| Aggregate error content | JSON-RPC error message/data bypassed result guardrails. Decoder diagnostics could echo upstream content without evaluation. | Structured JSON-RPC errors pass through the result policy. Invalid-response errors use a static public diagnostic. |
| Invocation logging | Several failures after authorization returned without an invocation record. Six branches duplicated log construction. | A single finalizer records the outcome of each authorized execution attempt, including preparation and client construction failures. |
| OAuth registry edits | Read responses omitted supported OAuth and discovery fields. Reading a server and then saving an unrelated edit could erase those settings. | The view preserves supported fields with type checks and excludes arbitrary secret blobs. A persisted read/update test covers the round trip. |
| OAuth redirects | URL parsing could remove raw control characters while the redirect retained the original value. | Redirect validation rejects control characters before URL parsing. This fixes invalid redirect output; the audit did not demonstrate header injection. |

The SSE behavior follows the [WHATWG event stream parsing rules](https://html.spec.whatwg.org/multipage/server-sent-events.html#parsing-an-event-stream). Response validation follows the [JSON-RPC response contract](https://www.jsonrpc.org/specification#response_object), which permits a result or an error, but not both.

Guardrail handling now has a separate module from direct proxy routing. Aggregate execution has a single lifecycle that owns arguments, result metadata, and final logging. The refactor removes redundant HTTP type conversions, trivial forwarding wrappers, and temporary upstream objects used only to build log records. It keeps domain helpers that define real boundaries, such as credential preparation and policy evaluation.

The tests now exercise serialized responses, loopback HTTP requests, and persisted state. The audit replaces an equality-wrapper test with callback checks for missing and mismatched browser sessions, and replaces a test that rebuilt tool definitions with checks of the actual `tools/list` response. Existing wire-format tests remain because they protect protocol behavior.

Final validation passed:

- `mise run rust-lint`: workspace Clippy for all targets, with warnings denied.
- `mise run rust-fmt-check`: workspace Rust formatting.
- `mise exec -- cargo test -p gateway -p gateway-mcp -p gateway-service --lib --quiet`: 563 passed; one existing manual request-truncation measurement harness ignored.
- `mise exec -- cargo test -p gateway-mcp --test streamable_http --quiet`: five loopback HTTP regressions passed.
- `git diff --check`: no whitespace errors. An independent source review confirmed closure of the reported policy and error-disclosure paths.

Validation uses local fixtures, not external MCP servers or OAuth providers. Strict enforcement of negotiated protocol-version support remains a compatibility decision: the existing local server and default client use different supported protocol dates. The audit fixes inconsistent initialization output without imposing a new support policy.
