# Gateway Guardrails Interview

- Date: 2026-08-22
- Status: Confirmed for issue planning
- Sources: [Destructive Command Guard](https://github.com/Dicklesworthstone/destructive_command_guard), [Amazon Bedrock ApplyGuardrail](https://docs.aws.amazon.com/bedrock/latest/userguide/guardrails-use-independent-api.html), [Google Cloud Model Armor](https://docs.cloud.google.com/model-armor/overview)

## Purpose

This interview defines the scope for gateway guardrails that inspect model traffic and tool execution. The result is an implementation-ready Wayfinder map and a set of scoped delivery issues.

## Existing Constraints

- Oceans can stop an MCP `tools/call` before an upstream server receives it. Both aggregate `/mcp` execution and direct `/mcp/{server_key}` proxying must use the same guardrail decision path.
- The LLM gateway can inspect a model-generated tool call, but it cannot stop a local process after a client receives the call. A local harness hook is required for execution-time shell enforcement.
- Streamed tool-call arguments arrive in fragments. A hard policy decision before release requires buffering guarded streams.
- Existing MCP invocation logs already distinguish allowed, denied, and not-evaluated policy outcomes.
- Destructive Command Guard has a custom license. Oceans can use its public design as evidence, but must not copy code or rule definitions without a compatible license grant.

## Confirmed Destination

Create an implementation-ready epic with scoped delivery issues, requirements, dependencies, and objective definitions of done. This effort carries implementation through the map instead of stopping after investigation tickets.

## Confirmed Policy Model

- Policies support `audit` and `deny`. The first release has no caller bypass, allow-once token, or permanent caller allowlist.
- Configuration is authoritative. The admin UI shows effective policies and events but does not edit policies.
- Global defaults can be overridden at the model-route and MCP-server levels. Team, user, and API-key policy scopes are out of scope.
- Deterministic checks run first. A deterministic deny stops evaluation. Managed checks then run in configured order, and any deny stops the operation.
- Managed-service errors and timeouts are configurable. The default is fail open with an audit event.
- Managed services can return masked or sanitized content. Oceans preserves an allowed transformed value and records that a transformation occurred.

## Confirmed Deterministic Packs

The first release includes built-in, versioned packs only:

- `core.shell`
- `core.git`
- `core.filesystem`
- `database.postgresql`
- `cloud.aws`
- `cloud.gcp`
- `saas.notion`

Shell packs use command-aware text patterns. MCP packs use structured selectors over the server, tool name, and JSON argument paths. The Notion pack blocks destructive page, database, workspace, move, and archive actions when the provider exposes such operations.

## Confirmed Enforcement Points

- Model prompts before provider invocation.
- Model responses before release to the caller.
- Model-generated shell tool calls, including complete buffering of guarded streamed responses and a stable policy-denied error.
- MCP calls and MCP results on both aggregate and direct MCP paths.
- A low-latency authenticated guard evaluation API used by a generic hook contract and first-party Pi and OpenCode adapters.

## Confirmed Managed Integrations

- Amazon Bedrock Guardrails uses the standalone `ApplyGuardrail` API and references a pre-created guardrail identifier and version.
- Google Cloud uses Model Armor prompt and response sanitization APIs and references pre-created templates.
- Both integrations are provider-neutral and attach to Oceans model routes rather than only to routes hosted in the same cloud.
- Oceans does not create, update, version, or delete cloud guardrail resources.
- Prompts, model responses, MCP calls, and MCP results can be inspected.

## Audit and Admin Contract

- Per-decision records keep policy, pack or managed-service identity, phase, action, reason code, latency, failure disposition, and content hashes.
- Raw inspected content is not retained by default. Payload retention follows the existing opt-in payload capture policy.
- The admin API and UI show effective read-only policy configuration, decision events, filters, and failure or transformation details.

## Out of Scope

- Operator custom packs.
- Porting every Destructive Command Guard pack.
- Caller bypasses and allowlists.
- Team, user, or API-key policy overrides.
- Cloud guardrail resource lifecycle management.
- First-party adapters beyond Pi and OpenCode.
- Editable guardrail policy controls in the admin UI.
