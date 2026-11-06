# Provider and API harness comparison

## Verdict

The main gaps are in **Chat translation, Gemini terminal-state handling, ordered reasoning replay, and Bedrock decoding**. The native Anthropic Messages boundary is the correct design: its tested native response retains data that the Chat projection loses. Do not apply SDK transcript repair to native payloads.

This document stores the research baseline and the correction outcomes for PR #611. The baseline observations below describe commit `0250f76352b9d50394ad7db8f8d05e629673e14d`, before the corrections. The implementation record at the end tracks the accepted scope; unverified leads and optional provider features are not promoted into confirmed defects.

## Scope and source pins

Oceans baseline: `0250f76352b9d50394ad7db8f8d05e629673e14d`, on `codex/anthropic-native-messages`.

| Trust rank | Reviewed source | Revision |
|---|---|---|
| 1 | [oh-my-pi, packages/ai](https://github.com/can1357/oh-my-pi/tree/2b70f15f090e15b97dd1d04458ae61f3b14b7eec/packages/ai) | `2b70f15f090e` |
| 2 | [pi, packages/ai](https://github.com/earendil-works/pi/tree/d9af3d73afb3b2c02c7a833d7ca9f3875e5fff6e/packages/ai) | `d9af3d73afb3` |
| 3 | [LiteLLM, litellm](https://github.com/BerriAI/litellm/tree/fca3d4f513001c6b9d2455206e9bfc8d26998170/litellm) | `fca3d4f51300` |

Workflowz used one explorer per source, then six independent refutation lenses. Workers shared findings and contradictions over IRC. Primary provider docs and local execution took precedence over agreement between harnesses.

## Corrections worth making

The observations below are from real Oceans provider code against local HTTP/SSE fixtures, unless noted. They prove the local behavior for those inputs, not that a live cloud model emits every tested combination. Provider paths are relative to `crates/gateway-providers/src/`.

| Order | Gap and observed behavior | Oceans location | Recommended correction and reference |
|---|---|---|---|
| First | **Gemini premature EOF becomes success.** After a function-call event, EOF without `finishReason` emits `tool_calls` and `[DONE]`. | `vertex/google_stream.rs:79-105` | Track observed upstream termination separately from accumulated tool calls. A call is output, not a terminal event. [pi checks an actual finish reason](https://github.com/earendil-works/pi/blob/d9af3d73afb3b2c02c7a833d7ca9f3875e5fff6e/packages/ai/src/api/google-vertex.ts#L191-L244). |
| First | **ID-less Gemini results can be attached to the wrong call.** For calls `f(a), f(b)` and Chat results `b, a`, the adapter emits anonymous results in `b, a` order. | `vertex/google_request.rs:156-205`; `vertex/google_tools.rs:205-235` | Resolve each result by its Chat `tool_call_id`, then emit complete result groups in call order where the native wire omits IDs. Preserve turn boundaries. [OMP sorts these results](https://github.com/can1357/oh-my-pi/blob/2b70f15f090e15b97dd1d04458ae61f3b14b7eec/packages/ai/src/providers/google-shared.ts#L329-L393). |
| First | **Gemini finish reasons are masked.** A prior call hides a later `MAX_TOKENS` or `SAFETY`; `MODEL_ARMOR` without a call becomes normal `stop`. Both JSON and streaming mappings have gaps. | `vertex/google_stream.rs:83-91`; `vertex/google_response.rs:81-107,155-170` | Keep truncation, filtering and invalid-call outcomes ahead of ordinary tool completion. Map documented reasons explicitly; retain the native reason. [pi preserves length before tool-use conversion](https://github.com/earendil-works/pi/blob/d9af3d73afb3b2c02c7a833d7ca9f3875e5fff6e/packages/ai/src/api/google-vertex.ts#L239-L244). OMP itself upgrades length to tool use, so rank 2 is better here. |
| Next | **Shared Anthropic Chat translation hides context truncation.** `model_context_window_exceeded` becomes `stop`, not `length`. | `anthropic/response.rs:168-175`; shared `anthropic/streaming.rs` | Map it to `length` for direct Anthropic, Vertex Claude and OpenRouter Claude Chat. Bedrock's separate mapper already handles it correctly. [OMP has the precise mapping](https://github.com/can1357/oh-my-pi/blob/2b70f15f090e15b97dd1d04458ae61f3b14b7eec/packages/ai/src/providers/anthropic.ts#L6999-L7010). |
| Next | **Converse malformed generation becomes normal completion.** Both `malformed_tool_use` and `malformed_model_output` become `stop` in JSON and SSE. | `bedrock/response.rs:685-694`; `bedrock/eventstream.rs:311-323` | Expose a failed generation and its native reason, not an ordinary successful finish. These are allowed AWS stop values, not necessarily HTTP errors. Keep consuming later usage metadata. [OMP treats other failure stops as errors](https://github.com/can1357/oh-my-pi/blob/2b70f15f090e15b97dd1d04458ae61f3b14b7eec/packages/ai/src/providers/amazon-bedrock.ts#L1571-L1584). |
| Next | **Explicit Gemini strict tools are weakened.** `strict:true` plus `tool_choice:auto` reaches the wire as `AUTO`, with no strict guarantee. | `vertex/google_tools.rs:239-299,330-381` | Honor strict semantics only for proven model/API/schema combinations; otherwise reject the explicit request. [pi selects `VALIDATED`](https://github.com/earendil-works/pi/blob/d9af3d73afb3b2c02c7a833d7ca9f3875e5fff6e/packages/ai/src/api/google-shared.ts#L473-L490), but this is not proof of full OpenAI-strict equivalence. `ANY` changes tool-choice semantics; mixed tool lists also need care. |
| Next | **Gemini signed native parts lose position and data.** A fixture with signed text A, signed text B, a signed call, then text C replays as merged ABC with only B's ordinary signature, followed by the call. | `vertex/google_response.rs:8-45`; `vertex/google_tools.rs:90-113` | Preserve an ordered native-part representation for extension-aware replay. Google says not to merge signed parts or move their signatures. Non-function signatures are recommended, not proof of a mandatory 400; call signatures are mandatory. [Primary signature rules](https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/thinking/thought-signatures). |
| Next | **Accepted Anthropic Chat extensions lose server-tool state.** Native web-search tools pass through, but server-tool blocks, result blocks, citations and `pause_turn` are lost in the Chat response. The same native Messages JSON is preserved exactly. | `anthropic/request.rs:592-600`; `anthropic/response.rs:19-31,140-175`; `anthropic/streaming.rs` | Either retain enough opaque state and citations for continuation, or reject unsupported native extensions on the Chat surface. Do not report a paused turn as fully done. [LiteLLM retains server-tool results and citations](https://github.com/BerriAI/litellm/blob/fca3d4f513001c6b9d2455206e9bfc8d26998170/litellm/llms/anthropic/chat/transformation.py#L2755-L2802). Native Messages is not affected. |
| Next | **Requested Converse response fields disappear.** The adapter forwards `additionalModelResponseFieldPaths`, then drops a returned `additionalModelResponseFields` object in JSON and `messageStop`. | `bedrock/request/inference.rs:90-99`; `bedrock/response.rs:3-69`; `bedrock/eventstream.rs:311-323` | Return the object in provider metadata, or reject the unsupported request. AWS may legitimately omit an absent path; the defect is losing values actually returned. [AWS request/response contract](https://docs.aws.amazon.com/bedrock/latest/userguide/conversation-inference.html). |
| Conditional | **Requested Gemini log probabilities disappear.** `logprobs` controls reach the provider, but returned `logprobsResult` is dropped. | `vertex/google_request.rs:53-55`; `vertex/google_response.rs:155-198`; `vertex/google_stream.rs:68-92` | Translate returned probabilities or reject the unsupported option. [LiteLLM has the useful proxy mapping](https://github.com/BerriAI/litellm/blob/fca3d4f513001c6b9d2455206e9bfc8d26998170/litellm/llms/vertex_ai/gemini/vertex_and_google_ai_studio_gemini.py#L3108-L3114). Gemini 3.x deprecates this feature; current model-specific support was not established. Do not assume old 2.5 models remain available. |
| Conditional | **Root-only cache aliases are ignored by accounting.** With 100 prompt tokens and 80 root-level cache hits, the current normalizer returns cache unavailable. | `crates/gateway-service/src/service.rs:1585-1640` | Add provider-scoped alias normalization with clear precedence. [DeepSeek documents hit/miss fields](https://api-docs.deepseek.com/guides/kv_cache); pi prefers canonical nested details. Current canonical nested DeepSeek/Kimi responses already work. If only aliases are present, configured-rate pricing can fall back to the full input rate; valid provider-reported cost still wins. No live ledger overcharge was measured. |

**Reachability caveat for Gemini filtering:** the stream fixture sends a call, then a separate empty-content terminal chunk. Google's docs say a SAFETY-blocked chunk has empty content. The code masks that later reason, but no live provider sequence was observed. A single JSON candidate containing both a call and SAFETY is not established as a real provider response.

## Known limitations and compatibility work

### Ordered signed reasoning replay

This is broader than a single missing signature field:

- **Bedrock Converse and Bedrock Anthropic Chat:** complete returned reasoning metadata is not read back into outgoing history. This is already documented under issue #140, not a new PR #611 regression. Native Messages is a different path.
- **Shared Anthropic nonstream Chat:** the normalizer splits thinking, text and tools. A fixture `[thinking1, tool1, thinking2, tool2]` replays as `[thinking1, thinking2, tool1, tool2]` even when the client keeps returned metadata.
- **Shared Anthropic Chat SSE:** ordinary thinking starts and native block indices are not exposed. Verbatim accumulated thinking/signature delta metadata is not accepted as complete replay blocks. A client can manually assemble a simple block; do not claim every stream is unrecoverable.

An ordered opaque replay representation is the needed contract. Simply prepending extracted thinking blocks is not enough. Both OMP and pi also fold some streamed Gemini content, so neither should be treated as a perfect arbitrary-part passthrough implementation. [Anthropic requires preserved thinking history](https://platform.claude.com/docs/en/build-with-claude/thinking#preserving-thinking-blocks); [OMP replays Bedrock signed reasoning](https://github.com/can1357/oh-my-pi/blob/2b70f15f090e15b97dd1d04458ae61f3b14b7eec/packages/ai/src/providers/amazon-bedrock.ts#L1413-L1454).

### Bedrock EventStream conformance

`bedrock/eventstream.rs:15-152` has two distinct gaps:

- **Both CRCs are ignored.** Already documented, but still contrary to Smithy. A correct CRC frame passed; independently invalid prelude and message CRC fixtures also delivered content and `[DONE]`.
- **Only header types 0, 1 and 7 are accepted.** A correctly checksummed frame with an extra legal timestamp header failed. Types 2–6, 8 and 9 are legal. Live Bedrock emission of such extra headers was not observed.

Prefer [OMP's custom decoder](https://github.com/can1357/oh-my-pi/blob/2b70f15f090e15b97dd1d04458ae61f3b14b7eec/packages/ai/src/providers/aws-eventstream.ts#L47-L129) as the rank-1 reference. It checks both CRCs and handles all legal types. OMP does **not** merely delegate this decoder to AWS SDK code. [Smithy requires both checksums](https://smithy.io/2.0/aws/amazon-eventstream.html#message-format).

Existing Oceans test frame helpers write zero CRCs. A conformance fix must replace those helpers with real checksums and include valid controls, not just corrupt-frame cases.

### Converse result grouping

The Chat mapper emits one user message per tool result. All three harnesses group consecutive results into one content array. Local probes confirm the outgoing shape, and [independent Bedrock reports describe validation failures](https://github.com/mlflow/mlflow/issues/24296). Coalescing is a sound compatibility correction, but this review did not prove universal rejection across all Converse models.

Do not transfer Anthropic's own same-role merge rule to Converse without evidence.

### Converse tool-call indices

A text block at native index 0 followed by the first tool block at index 1 becomes Chat tool index 1. The local probe confirms this. Map native block IDs to stable, dense tool ordinals, including interleaved argument deltas.

This is a client compatibility concern, not a proven universal OpenAI wire-rule violation. **[INFERENCE]** The [pinned OpenAI Python delta accumulator](https://github.com/openai/openai-python/blob/c6fc264689d384d5caec150fafa1a9567c4a3f56/src/openai/lib/streaming/_deltas.py) inserts sparse list entries while its [Chat stream consumer indexes by the original index](https://github.com/openai/openai-python/blob/c6fc264689d384d5caec150fafa1a9567c4a3f56/src/openai/lib/streaming/chat/_completions.py#L438-L444), which can fail for a first tool at index 1. That SDK was not installed or executed. An older reported AI SDK crash is fixed in later versions; do not present it as a current universal client failure. LiteLLM uses tool ordinals but its current-call tracking is not a safe model for interleaved deltas.

## Optional support, not existing-contract defects

- **Copilot dynamic vision header:** all three sources add `Copilot-Vision-Request:true` for image input; Oceans does not infer it. A configured static header is a workaround. No primary contract or live failure proved a universal requirement. Verify the image-detection rule and preserve configured overrides. [OMP reference](https://github.com/can1357/oh-my-pi/blob/2b70f15f090e15b97dd1d04458ae61f3b14b7eec/packages/ai/src/providers/github-copilot-headers.ts#L289-L300).
- **Vertex Express API-key mode:** separate endpoint/auth support, not a bearer-token rewrite of full Vertex. [Google Express contract](https://docs.cloud.google.com/gemini-enterprise-agent-platform/reference/express-mode/api-reference).
- **Direct Google API-key provider:** separate from Vertex and Express; currently a documented provider expansion.
- **Gemini multimodal tool results:** image/PDF function responses on supported models. Oceans explicitly rejects non-text Chat tool results. Standard OpenAI Chat tool messages permit text, so this is extension support, not a silent standard-Chat violation.
- **Gemini streamed function-call arguments:** currently rejected explicitly. Optional support, not accepted data silently lost.
- **New cross-family Responses bridges:** deliberate product/API contracts. A harness's agent abstraction or an adapter in the opposite direction is not proof that every gateway route must support `/v1/responses`.

## Claims rejected or held for more evidence

| Candidate | Assessment |
|---|---|
| “Separate adjacent Anthropic tool-result messages must fail.” | **Rejected as a proven Anthropic defect.** The [Messages API explicitly merges consecutive same-role turns](https://platform.claude.com/docs/en/api/messages/create). Grouping may help particular compatible providers; all three harnesses agreeing does not override the API contract. OMP's grouping code specifically mentions a compatible endpoint. |
| OpenRouter Claude Chat `response_format:json_object` | The local builder leaves this field on `/messages`; only schema mode is translated. Actual endpoint rejection, ignoring or enforcement is unverified. Do not silently strip it or invent an equivalent schema. [Native format schema](https://openrouter.ai/docs/client-sdks/typescript/models/messagesoutputconfigformat). |
| Gemini JSON without a finish reason, or empty JSON | Local probes produce `stop`; Google says absent finish means generation has not stopped. A malformed/incomplete-response hardening opportunity, not evidence ordinary cloud responses omit it. The valid candidate-less prompt-feedback block control returns `content_filter` and must remain valid. |
| Gemini v1 and function IDs on newer models | Oceans fixes the endpoint to `/v1` while model policy emits IDs for 3.5+. Google signature and function-calling docs differ by API/model version and ordering. Do not change IDs or switch versions without resolving that contract. The ID-less same-name bug above does not depend on this disputed case. |
| Gemini 4 `CONTINUATION` | Generic docs describe a v1beta1-only value; not established as reachable through the current `/v1` route. Not a current supported-route defect. |
| Rewrite Gemini JSON Schema `const` to `enum` | Present in pi, but the claimed provider need is not verified here. Do not copy schema rewrites merely because a comment says the provider ignores a keyword. |
| Fireworks cache-header fallback | OMP reads `fireworks-cached-prompt-tokens`; Oceans discards response headers before accounting. [Fireworks documents the metric](https://docs.fireworks.ai/serverless/overview), but current header-present/body-split-absent coexistence was not observed. Conditional nonstream support lead, not a measured billing error; do not assume stream headers exist or infer per-token dedicated-GPU billing. |
| Remove the 16 MiB EventStream cap | **Do not do this blindly.** It is a gateway resource policy versus protocol interoperability trade-off. Smithy has a separate 24 MiB payload service limit. The Oceans cap counts the whole frame; no live oversized frame was observed. Keep bounded-resource design explicit. |
| Inject OpenRouter `usage.include:true` | **Do not copy LiteLLM here.** [Current OpenRouter docs say usage is automatic and the old flags are deprecated](https://openrouter.ai/docs/cookbook/administration/usage-accounting). |
| SDK retries, WebSocket sessions, implicit cache breakpoints, orphan-tool repair or signature stripping | Not transparent proxy obligations. These can change cost, state, transcript ownership or security. No automatic adoption. Existing mixed-TTL pricing policy stays unchanged. |

## Coverage and limits

| Surface | Covered | Not established |
|---|---|---|
| OpenRouter | Claude native Messages selection, Chat bridge, Responses boundary, structured output, cache/cost handling and stream guards | Live native JSON-object behavior; exhaustive provider/plugin policies |
| Anthropic | Native versus Chat history, signed thinking, tools, citations, pause and context stops, JSON/SSE | Every beta/header/model feature or a full Claude Code session |
| Bedrock | Converse, ConverseStream, Anthropic Invoke/Mantle boundaries, parallel results, reasoning, stop/usage/extra fields, binary decoding | Live AWS validation; SigV4, credential and region matrix; every model |
| Vertex | Gemini and Claude mapping, history, tools, signatures, strict mode, log probabilities, terminal outcomes, full-Vertex versus optional Express | Live model/version behavior; full media/grounding catalog |
| OpenAI Chat/Responses | Distinct request families, reasoning/extensions, terminal/error guards, usage aliases, SDK-only differences | Every hosted tool, realtime/WS, Azure deployment or OAuth flow |
| Copilot and pricing | Vision-header comparison; cache normalization and reported-cost precedence | Live Copilot requirement; measured cloud billing impact |

The research-only pass made no live or paid provider requests, ran no full admin-stack or test/lint suites, and changed no runtime code. The implementation and current proof results follow below.

### Research execution evidence

The source audit used three disposable Rust programs with the real public provider clients and local HTTP, SSE and properly checksummed AWS EventStream fixtures. They established the baseline behaviors in the tables above. Responses controls preserved encrypted reasoning and extension events, accepted `response.incomplete`, and rejected premature EOF. The native Messages JSON control preserved the full server-tool response.

Cache-alias fixtures used a verbatim copy of the private accounting normalizer, not an end-to-end ledger run. Raw-response forwarding, ledger normalization and reported-cost precedence were source-traced separately. No live or paid provider requests were made during that research pass. Disposable fixtures were removed. Implementation regression tests and current runtime evidence are recorded below.

## Implementation order

1. Correct terminal truth and stop mapping, plus ID-less tool-result association. Keep valid prompt blocks and post-stop usage accounting.
2. Honor or explicitly reject requested controls and returned fields; normalize proven cache aliases without overriding canonical data or provider-reported cost.
3. Close ordered-replay and EventStream conformance gaps as explicit provider work. Keep native Messages opaque; do not build a global transcript-repair layer.
4. Treat Copilot, new Google auth modes, multimodal results and new API-family bridges as separate support decisions. Hold unverified wire behavior until the provider contract or a scoped canary proves it.

## Correction implementation record

| Correction | Implemented result |
|---|---|
| Terminal truth | Gemini requires a valid terminal outcome, preserves length/filter precedence over tool calls, and fails incomplete or malformed replies. Bedrock rejects malformed stop reasons. Streaming failures retain late usage and do not emit a successful `[DONE]`. Anthropic context limits map to `length`. |
| Tool association and controls | Complete ID-less Gemini result groups retain the original call order, including repeated tool names. Bedrock groups parallel results and emits stable, dense tool indices. Unsupported Gemini strict tool/schema requests fail explicitly rather than being silently weakened. |
| Ordered replay | Gemini, Anthropic Messages and Bedrock Converse retain ordered native parts, signed/redacted thinking, tool state and future blocks in provider metadata. Source tags prevent replay across incompatible transports. Changed public text, tools or block controls invalidate stale replay. Native Messages does not use this Chat envelope. |
| Guardrail safety | Output transformations invalidate native replay and signature-bearing metadata across the whole JSON response or buffered stream, including late complete snapshots. Regression tests first exposed the original-content leak, then passed with the fix. |
| Returned fields | Gemini retains requested log probabilities. Converse retains `additionalModelResponseFields`. Anthropic Chat retains server-tool, citation and pause continuation state. |
| Binary transport | AWS EventStream validates both IEEE CRC32 checksums and all typed-header bounds, including timestamp headers. A corrupt frame cannot erase valid usage from an earlier frame in the same network chunk. The 16 MiB resource ceiling remains. |
| Cache accounting | Ordinary Chat accepts the verified root-level cache aliases and DeepSeek hit/miss counters. Explicit canonical fields, including zero or invalid values, retain precedence. Misses are not cache writes. Raw usage, provider-reported cost and the distinct native Messages/Responses rules remain intact. |
| Valid PR feedback | Native SSE model rewriting now splices only the structural model value and preserves every other JSON byte and SSE framing detail. Successful native JSON replies parse traced bytes directly, without an intermediate `String`; strict UTF-8, truncated-body and deadline failures remain visible. |
| Complete native tool input | Stream guardrails inspect nonempty complete start-frame inputs independently from later deltas. Conflicting initial and delta inputs cannot hide one another. Transforms retain object-shaped inputs, and empty opening objects keep their normal delta behavior. |
| Image cache policy | Chat image translation retains outer cache controls and TTLs for remote string/object URLs, data URLs and base64 sources. Remote URLs keep the existing OpenRouter opt-in and transport restrictions. |
| Strict tool controls | The [strict-loss finding](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4487540335) was valid. Chat function booleans become native tool-level `strict`; Chat null and omission stay unset. The matching reverse gap is also fixed: native `strict` becomes `function.strict` on generic Chat routes. Schemas are not changed; owned fields move rather than clone. |
| Compaction continuation | Both Anthropic Chat stream adapters accept complete compaction snapshots, preserve null summaries and encrypted checkpoint presence, retain following text, and replay the ordered block. Malformed values or boundaries fail without success while draining late usage. |
| Replay ownership | Terminal assembly moves Anthropic and Converse block sequences and message fields into metadata instead of cloning full snapshots. Parsed tool-input buffers and failed replay state are released. Inline continuation still requires one response-sized sequence. |
| Bedrock malformed replay containers | Known tool and reasoning containers are checked before mutation. An assembly failure is retained through later stop diagnostics, usage and EOF, without reopening content or reporting success. Opaque partial deltas merge without losing tool identity, accumulated input or future fields. |
| Gemini result-order bounds | A borrowed rank map replaces repeated linear ID scans. Reordering requires a one-to-one match between unique calls and unique results. Duplicate or incomplete groups retain input order, including results whose IDs appeared in an earlier turn. |
| Shared route policy ownership | The unchanged OpenRouter native-route predicate moved from `claude_models` to the existing shared `route_capabilities` module. Dispatch and advertised metadata still use one authority; no provider SDK dependency, alias or second policy was added. |

Native Messages remains opaque. New provider/auth families, unverified JSON-object behavior, speculative schema rewrites, SDK transcript repair, and pricing-schema expansion remain outside these corrections. The existing EventStream resource ceiling remains in force.

The corrections were implemented in parallel workflowz pools, then reviewed in a separate four-reviewer pool. Review findings were checked against source and executable fixtures before fixes were accepted. The review also caught signed-replay projection mismatches, late-usage loss, partial tool-JSON handling, and coalesced-frame corruption cases.

Two new PR findings arrived after the first correction commit. Both were reproduced before production changes: five initial regressions failed on the tool-input bypass and lost image cache policy. Separate workflowz test and implementation pools then corrected both boundaries. All seven final regressions passed, including conflicting initial/delta inputs and object-preserving transforms.

Two further comments were found during closeout. A second workflowz pool split compaction contract/regression work from replay ownership changes. The compaction success cases failed before the fix, while malformed-input rejection already passed. All three consumer regressions then passed across both adapters.

The compaction fix follows the [official complete-summary wire contract](https://platform.claude.com/docs/en/build-with-claude/compaction-threshold#streaming) and the [pinned primary SDK accumulator](https://github.com/anthropics/anthropic-sdk-typescript/blob/832402e8036a2815c9bc13cdce2373b79b0e96cb/src/lib/BetaMessageStream.ts#L806-L815). Summary values replace, not append; explicit null remains null. An absent encrypted checkpoint preserves the prior value, while explicit null clears it. The review's memory concern was fair for avoidable terminal copies, but disabling replay would remove the current continuation contract. No constant-memory or measured memory-saving claim is made.

The strict contract was checked against pinned primary [Anthropic](https://github.com/anthropics/anthropic-sdk-python/blob/4c7b598466b88559583f98216139be8e9b34c1fc/src/anthropic/types/tool_param.py), [OpenRouter](https://github.com/OpenRouterTeam/typescript-sdk/blob/fe342e9289ff54b78fa367d03358a94d29ad94c1/src/models/messagesrequest.ts#L1556-L1577) and [OpenAI](https://github.com/openai/openai-python/blob/c6fc264689d384d5caec150fafa1a9567c4a3f56/src/openai/types/shared_params/function_definition.py) schemas. Both translation directions failed before their fixes. Only the two behavioral conversion regressions were retained; duplicate wire-builder and native-forwarding tests were removed in favor of real transport smoke checks.

The [native JSON guardrail finding](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4487571361) was refuted. The public JSON handler already awaits `guard_model_response`, whose generated-tool collector walks the full object and parses top-level `tool_use.input`; it is not limited to Chat choices. The real TCP proof returned a native 403 before releasing dangerous tool content, and the production admin API recorded a `generated_tool_call` denial from `core.filesystem`. Safe native JSON retained its fields, extensions and input object. No production guardrail change was needed. Managed transformation services were not emulated; native object replacement remains covered by the existing consumer regression.

Four more comments were assessed by four parallel reviewers before changes. The [Bedrock container finding](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4487772311) was valid: three malformed shapes panicked, and an opaque tool delta lost the earlier identity and input. All four consumer regressions pass after the fix. Real provider HTTP/EventStream checks also passed 18 malformed-container cases and two opaque continuation cases, with split and coalesced server bodies. Each failure retained late usage before one structured error; stop diagnostics survived, and no success or replay snapshot was emitted.

The [Gemini repeated-scan finding](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4487782734) was valid. Investigation also found a correctness defect: duplicate call IDs could move an unrelated earlier-turn result ahead of its peer. The real HTTP baseline observed that wrong order; the corrected HTTP/SSE probe preserved the ambiguous group and ordered 2,048-result valid groups. All 38 Google tool tests pass. The new regression uses structured result objects so it does not assert an incidental plain-string wrapper. Local timing samples are retained only as probe diagnostics, not a general benchmark.

The [OpenRouter ownership comment](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4487782740) was fair about module cohesion, not a demonstrated routing defect. The predicate now lives beside shared route capability policy. Moving it into `gateway-providers` would add its SDK and auth dependencies to `gateway-service`, which does not depend on that crate. Core already owns route compatibility data; the existing `url` dependency remains.

The [separate native capability trait proposal](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4487782745) was not adopted. `ProviderClient` already has protocol-specific optional operations with typed unsupported defaults, and the registry stores one `Arc<dyn ProviderClient>`. Native support varies by request route within each provider, so trait membership cannot replace the contextual capability check. A separate accessor would add a dispatch convention without fixing a demonstrated defect. The ADR records this trade-off; no downcast, boxing layer or fallback shim was added.

### Verification results

| Check | Observed result |
|---|---|
| Initial Messages gate | 2,165 tests and all 78 required checks passed. |
| Boundary-fix Messages gate | 2,172 tests and all 85 required checks passed. |
| Replay-closeout Messages gate | 2,175 tests and all 88 required checks passed. |
| Strict-closeout Messages gate | 2,177 tests across gateway, providers, core, service and client-config, and all 90 required checks passed. Five existing tests remained ignored; no required regression was ignored or missing. |
| Final container/order Messages gate | 2,182 tests and all 95 required checks passed. The final verifier helper run passed all 37 tests, and `mise run lint` passed again. |
| Verifier helper tests | 37 passed, with no failures or skips. |
| Workspace lint | `mise run lint` passed, including Rust workspace targets with warnings denied, UI and review-agent checks, formatting, links and secret scanning. Existing UI lint warnings remain. |
| Public provider-client smoke | 16 executable cases passed through real provider clients against local HTTP, SSE and independently checksummed EventStream fixtures. These covered Gemini terminal/replay/association controls, Bedrock stop/replay/binary behavior, and OpenRouter Anthropic Chat/native fidelity. |
| Distinct Responses controls | Three executable controls passed: encrypted input and JSON fidelity, incomplete-terminal continuation with future events, and premature-EOF failure without a success marker. |
| Follow-up public gateway smoke | Six cases passed through the real TCP listener, authentication and request pipeline with a local native upstream: three dangerous complete/conflicting tool inputs returned 403 before release, two safe streams retained native bytes and completion, and Chat image cache policies survived translation. The fixture key was revoked and then rejected with 401. |
| Replay-closeout public provider-client smoke | Five local transport cases passed: summary/null compaction and request continuation through OpenRouter and Bedrock Mantle, plus a large Converse replay with late usage. These use real provider clients and HTTP/SSE/checksummed EventStream, not live cloud calls or a memory benchmark. |
| Strict provider-client smoke | 20 real HTTP/SSE cases passed below gateway configuration validation: eight OpenRouter Chat, six OpenRouter native, and six native-to-generic-Chat cases. These covered both stream modes, explicit booleans, nullable Chat controls, omission and unchanged schemas. |
| Native JSON and strict public gateway smoke | Seven cases passed through the real TCP listener, password onboarding, scoped-key and policy APIs: denied top-level native tool input, safe native JSON, native strict/extension fidelity, and four Chat strict-control states. The inference-only key was revoked and then rejected with 401; both fixture servers were stopped and awaited. |
| Final Bedrock provider HTTP smoke | 18 malformed-container cases and two opaque continuation cases passed with independently checksummed EventStream frames, split/coalesced server bodies, late usage, stop diagnostics, missing terminals, and no false success. No live AWS call was made. |
| Final Gemini provider HTTP/SSE smoke | Three JSON captures and one stream capture preserved the order of 2,048 results. A separate ambiguous-history capture changed from incorrect order before the fix to preserved input order after it. No live Vertex call was made. |
| Container/order admin proofs | All nine planned feature IDs passed `evidence --require-current` before and after stack teardown. The observability driver also proved Agent Harnesses. Browser checks covered Models, Leaderboard, password sign-in, Request Logs, live requests, native Messages, backend gateway, batches and rate limits. |

Runtime run: `20261105T225834Z-pr611-provider-corrections`, gateway `0.48.0`. Evidence remains at `/tmp/oceans-admin-verification/20261105T225834Z-pr611-provider-corrections/evidence`. The source-fingerprinted report records optional failures and missing integration prerequisites separately from required checks.

Earlier passes used two bounded live calls each: the backend driver's forced-tool Chat canary through OpenRouter, and one native Messages stream through OpenRouter to `anthropic/claude-haiku-5.5`. Native streams completed with `end_turn`, 17 input tokens, 4 output tokens, and provider-reported cost `$0.0000037`. Gateway request IDs were `c9a3e190-3ae3-401b-949d-ea74ebeb7042` initially, `a15ce650-f547-49b3-b1ee-3f13c12593f7` after the boundary fixes, and `f51c02a7-77df-4183-9df2-d8f98c439317` after final Rust formatting. The Request Logs UI and production API matched each successful terminal provider attempt and its usage. The four-decimal cost display rounded the small amounts to zero.

Replay closeout reused the still-current backend proof and made only one native canary, shared by the Messages, live-request and refreshed Request Logs proofs. Request `d716057c-2fd6-416c-8370-7e503dac652c` completed with the same stop, usage and cost. Current safe UI ARIA and production API fields matched. Image capture timed out with browser animation clocks frozen at zero; prior screenshots were retained as earlier evidence, not relabelled as fresh captures.

Strict closeout again shared one native canary across those three proofs. Request `be3fe7d5-ad3c-4dcd-a5df-abd09452cf5f` completed with `end_turn`, 13 input tokens, 4 output tokens and reported cost `$0.0000033`. Safe Request Logs UI regions and the production API matched the route, usage and terminal successful attempt. The UI-created single-model inference key was revoked, subsequent Models access returned 401, and the browser session was signed out. Screenshot renewal was not retried after the earlier capture-engine failure; only fresh safe ARIA and API evidence is claimed.

The final container/order pass renewed Models through the maintained Playwright driver, including fresh screenshots and UI/API parity. One native canary, shared across Messages, live requests and Request Logs, completed as request `665da799-d5ea-4413-9a58-04af0d9cd441` with `end_turn`, 13 input tokens, 4 output tokens and reported cost `$0.0000033`. Its UI/API route, usage and successful terminal attempt matched. The inference-only single-model key was revoked and then rejected with 401; the browser was signed out and closed. All nine planned proofs passed `--require-current` before and after owned-stack cleanup. Five unaffected proofs kept their still-current source fingerprints instead of making more paid calls.

The initial native canary consumed a temporary one-request-per-minute key policy. Its immediate repeat returned HTTP 429, `rate_limit_error`, and `Retry-After: 32`; no second paid Messages call was made in that pass. Policy edit, pause and deletion were verified through the UI and API. Temporary live keys were revoked in all passes, and each native key then returned HTTP 401. The queued demo batch was not cancelled. Its actual POST server-function polling was observed 5,121 ms apart. Browser sessions were signed out and the run-owned stack was stopped.

The follow-up renewed the source-stale backend, native Messages and live-request proofs, and refreshed Request Logs with the same canary. The other five planned feature proofs retained current source fingerprints; they were not relabelled as new runs. Final formatting required another gate and backend/Messages renewal. Sanitized artifacts include `boundary-public-smoke.json`, `followup-native-observations.json`, `final-native-observations.json`, `late-replay-public-client-smoke.txt` and `closeout-native-observations.json`. All owned disposable fixture files were removed.

Strict closeout added `strict-provider-client-smoke.json`, `native-json-guard-public-tcp.json`, `strict-closeout-native-observations.json` and `strict-request-log-api-safe.json`, plus safe UI ARIA and cleanup records. The initial public fixture key request incorrectly supplied MCP grants without an MCP scope; that disposable setup was corrected to omit MCP grants, without widening the inference-only key.

Final artifacts include `gemini-order-before.json`, `gemini-order-after.json`, `bedrock-container-http-smoke.json`, `final-native-observations.json`, `final-request-log-api-safe.json`, safe UI ARIA and key/browser cleanup records. The final disposable transport fixture was removed. Manual native-path screenshots were not retried through the failed managed-browser capture path; fresh Models screenshots and fresh manual ARIA/API evidence are identified separately.

The run ignored the gitignored `mise.local.toml` through `MISE_IGNORED_CONFIG_PATHS` to avoid duplicate task-secret environment sources. No local secret configuration was edited. Proof artifacts omit keys, cookies, prompts and responses; request-log and batch payloads were excluded or redacted.

### Structural ownership closeout (2026-11-06)

The approved structure-only pass keeps the existing crate graph and crate-root provider facade. Provider-port contracts now have one owner in `gateway-core::provider`; persistence stays in its existing modules. Pure usage interpretation lives in the service usage leaf, separate from pricing and persistence. Public Messages dispatch now shares the inference lifecycle without changing native headers, error shape, terminal guards, request identity or accounting. The [ownership ADR](../adr/2026-11-03-native-anthropic-messages-transport.md#amendment-provider-ownership-2026-11-06) records the module boundaries and trade-offs.

The shared Anthropic compatibility stream coordinator belongs to Anthropic, not Bedrock. Bedrock binary decoding, Converse stream delivery and ordered replay have separate owners. Vertex content, conversation and replay mapping no longer form a request/tools dependency cycle. Replay failures use a private typed error; Google stream response mapping returns its events directly. No new crate, fallback shim or broad test/comment cleanup was added.

Four independent reviewers checked the ownership slices. One stale private test import was corrected. The final Messages gate passed 2,182 tests and all 95 required checks. Final-tree public-client HTTP/SSE/EventStream smokes passed 52 Bedrock cases and the Vertex replay, ID-binding, terminal, late-usage and 2,048-result ordering cases. The fixture sources, manifests and lockfiles were removed after execution; no cloud Vertex or Bedrock proof is implied.

Two further PR comments were valid. The [Converse timing comment](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4488165748) corrected the docs: `messageStop` emits finish state and any usable replay snapshot, later metadata can still carry usage, and EOF appends `[DONE]` only after final checks. The [extra-frame comment](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4488179560) was reproduced through real Anthropic-compatible and Bedrock Mantle Chat clients. Each visible content delta now carries its matching raw native event in the same frame. Opaque-only events remain separate; native Messages passthrough is unchanged.

Both Chat adapters produced the same counts on the 64-text-delta HTTP fixture:

| Observed stream item | Before | After |
|---|---:|---:|
| JSON SSE frames | 132 | 68 |
| Combined text/native-event frames | 0 | 64 |
| Visible text deltas | 64 | 64 |
| Raw native events | 66 | 66 |

This proves lower frame volume with preserved visible output and raw event order, not a CPU, latency or memory benchmark. One existing assertion expected an exact serialized object ending after `delta.content`; it now reads the JSON value so the added metadata does not break a content check.

The final stream-slice review found no merge-related defect after the frame fix. The verifier helper suites run in this pass passed all 27 tests, and `mise run lint` passed. No full workspace test or PostgreSQL integration run is claimed.

Runtime run `20261106T193928Z-pr611-structural-refactor` used gateway `0.48.0`. All nine planned feature proofs passed `evidence --require-current` before and after owned-stack teardown; the observability driver also proved Agent Harnesses. Fresh Playwright screenshots and safe ARIA/API fields cover the native request log, alongside Models, rate limits, batches and password-session flows. Payloads were masked or omitted.

This pass made two bounded live calls: the backend driver's forced-tool Chat request `934ab27f-d881-4fe9-8cd3-e361b93fcfdc` and native Messages request `9a923c8f-f6ab-436b-93b7-41c7fc1272dd`. The native stream ended with `end_turn` and `message_stop`, 13 input tokens, 4 output tokens and reported cost `$0.0000033`. Its route, stream flag, usage and one successful terminal attempt matched the UI and API. The same canary proves Messages, live requests and Request Logs; no extra call was made for each feature.

The immediate repeat returned a free native `429` with `rate_limited` and no request-log row. The key-scoped policy was edited, paused and deleted through the UI, with API parity. Free hard-budget probes on Chat, Responses and Messages each returned `422` with `budget_exceeded`, null tokens/cost and zero provider attempts. Both manual keys were revoked and then returned `401`; the backend driver also completed its key cleanup. The queued demo batch was kept, not cancelled. Actual `getBatches` POST polling was observed 5,005 ms apart without an intervening action. Browser sessions ended and all owned browsers, fixtures and stack processes were closed.

Current artifacts are under `/tmp/oceans-admin-verification/20261106T193928Z-pr611-structural-refactor/evidence`, including `bedrock-structural-smoke.json`, `vertex-structural-smoke.json`, `structural-native-observations.json`, `structural-request-log-api-safe.json`, `structural-budget-observations.json` and `structural-manual-cleanup.json`. The two known Messages gaps and missing optional auth seeds remain visible in the proof report.

### Review-comment closeout (2026-11-06)

Six new unresolved comments were checked against source and executable regressions before their advice was accepted:

| Finding | Assessment and correction |
|---|---|
| [Replay text owns the public replacement](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4497390725) | Valid. Metadata-first streams lost the transformed answer; public-first streams duplicated transform input. Public text and replay copies now have separate ownership. Matching copies avoid duplicate inspection, while conflicting or unprojected text and tools retain their checks. |
| [Compatibility EOF can report success](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4497426490) | Valid. Mantle and Copilot now require actual `message_stop`, including recoverable incomplete replay. Native errors after replay failure remain errors, with observed late usage retained. |
| [Compatibility totals exclude cached input](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4497426499) | Valid. Merged native counters use the shared checked total. Overflow preserves counters and explicitly invalidates the numeric total with null instead of panicking or retaining an older value. |
| [Messages failure metrics are skipped](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4497426509) | Valid. Selection with no eligible provider, and stream-start failures, record one shared request metric. Existing tool-cardinality exclusions and no-charge behavior remain unchanged. Earlier model-resolution failures stay outside this lifecycle, matching Chat. |
| [Converse redacted prefixes are rebuilt](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4497430305) | Valid by source. Decoded fragments now append to per-block byte buffers and encode at terminal assembly. The continuation regression also exposed and corrected a dropped initial `data` alias prefix. |
| [Native preambles repeatedly move the tail](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4497430310) | Valid by source. Offset scanning releases complete prefixes per input chunk; it no longer drains and shifts the remaining buffer per frame. Coalesced and fragmented preambles retain their bytes. |

The initial baseline had eleven failing consumer regressions; denial, invalid-fragment and byte-preservation controls also ran. Two independent reviewers then found gaps in the first fixes. Three further failing regressions showed that interrupted Bedrock reasoning must reconstruct fragments separately from terminal snapshots. The corrected reasoning, public-text, denial, metrics and framing checks passed all fourteen targeted HTTP regressions.

The second review found that omitting an overflowing total alone leaves an earlier total in the production collector. The revised producer regression failed before explicit null invalidation. Real-collector controls confirmed that the existing merge clears null and uncached overflow still fails. Further consumer checks exposed a cache-overflow fallback that reported a partial total of two. Logs and billing summaries now preserve an explicit unknown instead; missing totals retain normal derivation, and invalid-cache pricing keeps its existing policy. No generic collector merge policy was changed.

The maintained Messages gate now requires the new public/replay ownership, interrupted reasoning, terminal, cache, overflow, collector, redacted continuation, metrics and preamble regressions. Local transport fixtures do not certify live cloud credentials or measured performance.

Final verification passed 2,205 Messages-gate tests and all 119 gate checks, all 37 verifier-helper tests, and `mise run lint`. Both independent reviewers reported no remaining findings. Disposable real-client transport proofs passed 22 provider cases across 28 HTTP requests and 14 public-gateway TCP cases. These exercised SSE, checksummed Bedrock EventStream, the production usage collector, managed guardrails, incomplete streams, redacted continuation and request metrics. Their source files, manifests and lockfiles were removed after execution.

Runtime run `20261106T211701Z-pr611-review-stream-fixes` used gateway `0.48.0`. All nine planned feature proofs passed `evidence --require-current`; the observability driver also proved Agent Harnesses. Two bounded paid requests were made: the backend driver's forced-tool Chat canary and one native OpenRouter Messages stream, shared by Messages, live requests and Request Logs. Native request `8207fdb2-b488-4aa4-b740-5bc4cd0b7ccf` ended with `end_turn` and `message_stop`, 13 input tokens, 4 output tokens and provider-reported cost `$0.0000033`. Fresh masked screenshots, safe ARIA and production API fields matched its route, usage and one successful terminal provider attempt.

The immediate repeat returned a free native `429` with `rate_limited` and `Retry-After: 7`, without a request-log row. Policy edit, pause and deletion matched the API. The inference-only, single-model key was revoked and then rejected with `401`; both browser sessions were signed out. The queued demo batch was kept, and actual `getBatches` POST polling was observed 5,057 ms apart. The owned browser and stack were closed. Evidence remains under `/tmp/oceans-admin-verification/20261106T211701Z-pr611-review-stream-fixes/evidence`, including `messages-regressions.json`, `provider-review-wire-smoke.json`, `gateway-review-tcp-smoke.json`, `review-native-observations.json` and sanitized admin proof artifacts. Known optional gaps remain listed below and in the report.

### Native text and model-boundary review (2026-11-06)

Three later comments were assessed against source rather than accepted as instructions:

| Finding | Assessment and correction |
|---|---|
| [Native text split across blocks](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4498148552) | Valid. Phrase-sensitive denial and transformation both failed before the fix. Typed native text starts and deltas now share message-scope inspection, while their original locations support replacement. Reasoning remains separate; unchanged frames retain their bytes. |
| [Private model-name matching](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4498171617) | Partly valid. `private-claude-opus-5-5` was misclassified and its manual controls were rejected. Classification now requires a token-prefix boundary as well as the existing documented suffix. The cited `-custom` example was already rejected, and the cited generic marker helper checks only suffixes; that helper was not changed. Documented OpenRouter, Bedrock and Vertex names retain their policies. |
| [No-route usage assertion](https://github.com/ahstn/oceans-llm/pull/611#discussion_r4498172714) | Valid test gap, not a reproduced charge. The existing gate-selected no-compatible-route test now checks that the authenticated owner's request ledger is absent for Chat, Responses and Messages, in both stream modes. All six cases passed. |

Before production changes, two native cross-block regressions, private-name classification and native caller-control preservation failed. The corrected four native stream cases, all nine model-classification cases and the provider control case passed.

The real TCP proof then exposed a related framing gap: legal LF-plus-CR blank-line separators passed the guard but prevented the public model rewrite. The shared SSE parser also missed those separators. Both mixed-delimiter consumer regressions failed before a narrow correction to the two existing scanners. All transport splits now retain the expected events and bytes. No new public helper or module boundary was added. Nine additional named regressions are required by the Messages gate.

The final disposable TCP proof passed all 17 cases through the production router and actual Anthropic-compatible and ModelArmor HTTP clients. It covered denial, replacement and unchanged output across blocks, start text and deltas, LF and mixed endings, reasoning and terminal usage, rejected-request ledger absence in both stream modes, and private-model control preservation. Keys were revoked, both listeners were stopped and awaited, and no cloud call was made. The sanitized artifact is `cross-block-public-wire.json` in the existing run's evidence directory.

Final checks passed 2,213 Messages-gate tests, all 128 gate checks, all 37 verifier-helper tests and `mise run lint`. An independent reviewer found no remaining defect in the text grouping, model boundaries or no-charge assertions. The real wire proof and all-split parser checks also passed after the mixed-delimiter correction.

The existing fingerprinted run renewed Models, backend gateway, native Messages, live requests and Request Logs. Four unaffected planned proofs retained their current source fingerprints; they were not relabelled as fresh checks. All nine planned feature IDs passed `evidence --require-current` after teardown. This pass made two bounded paid calls: the backend driver's forced-tool Chat canary and native Messages request `e01ff100-f001-43b2-ad4e-7b2546ba770b`. The native stream completed with `end_turn`, `message_stop`, 14 input tokens, 4 output tokens and reported cost `$0.0000034`. Fresh masked screenshots and safe ARIA/API fields matched its route, usage and successful terminal attempt. The single-model inference key was revoked and then rejected with `401`; the browser session, owned browser and stack were closed. All owned disposable fixture sources, databases and binaries were removed. Known optional gaps remain explicit in the report.

### Remaining verification limits and known gaps

- Live Vertex and Bedrock cloud calls were not exercised. Their local transport and replay checks do not prove cloud credentials, regions, signing or model-version behavior.
- No full authenticated Claude Code session was exercised.
- Both public Messages paths still return a plain-text HTTP 400 for malformed JSON instead of an Anthropic error envelope.
- Native Messages request logs still record and display `chat_completions`. The UI matches the stored metadata, but the native operation label is missing.
- Forced password rotation and the no-console-access state require seed conditions absent from this run.
- Mixed-TTL cache pricing keeps the existing policy. The related reviewer accepted the aggregate-counter fix and withdrew the request for a broader pricing-schema expansion.
- Inline Chat replay remains proportional to response size. Required legacy reasoning fields and SSE serialization still need memory; bounded/spooled replay would require a separate wire-contract design.
