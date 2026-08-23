# ADR: Gateway Guardrails Domain And Composition

Date: 2026-08-22

## Status

Accepted

## Decision

Oceans uses `crates/gateway-guardrails` as the primary domain boundary for guardrails. The crate owns protocol-neutral policy types, effective-policy resolution, guard phases, decisions, deterministic pack evaluation, structured MCP selectors, managed evaluator interfaces, managed-service adapters, and ordered composition.

The crate does not own Axum routes, HTTP authentication, YAML file loading, MCP transport, persistence, admin handlers, UI code, or harness process execution. Those components adapt their wire or storage types to the narrow types exported by `gateway-guardrails`.

Configuration is authoritative. One global policy supplies defaults. A model route or MCP server can replace individual global policy fields. The first release has no team, user, API-key, or caller-controlled policy scopes. Model-route keys use `{model}/{provider}/{upstream_model}`. MCP overrides use the registered server key. Startup validates all references after configured data and the MCP registry are available.

Each operation follows one order:

1. Resolve one effective policy.
2. Run deterministic evaluators first.
3. Stop on a deterministic deny.
4. Run enabled managed checks once in configured order.
5. Pass each allowed transformed value to the next check.
6. Stop on any managed deny.
7. Return the final original or transformed value with the ordered decision chain.

`audit` records a matching decision and permits the operation. `deny` records and stops it. Managed failures have a per-check disposition. The default is `fail_open`, which permits the operation and emits an audit decision. `fail_closed` converts the failure to a deny. Timeouts and content-size failures use the same disposition.

Decision records retain only privacy-safe metadata by default: decision ID, phase, effective scope, evaluator or managed service, pack and rule identity, action, stable reason code, latency, failure disposition, transformation flag, and SHA-256 content hash. Raw prompts, commands, tool arguments, and results follow the existing explicit payload-capture and redaction policy. Managed-service assessment metadata must not add raw content to default records.

## Implementation

`gateway-guardrails` exposes typed `EvaluationInput` values for prompts, model responses, generated tool calls, MCP calls, MCP results, and harness pre-tool checks. `GuardrailEngine` composes synchronous deterministic evaluators with asynchronous managed evaluators. Managed cloud response types are normalized before they enter this engine.

Built-in packs have stable IDs and semantic versions. Shell evaluation uses a command lexer and invocation model so quoted command text used as data does not become an executable match. MCP evaluation uses parsed JSON values, typed JSON paths, server and tool identities, aliases, and operation predicates. It does not apply a regular expression to serialized JSON.

The built-in patterns and fixtures are clean-room work derived from the confirmed product contract and public command or provider documentation. No Destructive Command Guard source code or rule definition was copied.

Gateway HTTP paths own canonical protocol adaptation and enforcement. Guarded streams must be buffered at the HTTP boundary before any bytes are released. Persistence repositories store decision metadata after evaluation. Harness adapters call the authenticated gateway endpoint immediately before process creation.

## Rationale

One domain crate prevents direct MCP, aggregate MCP, inference, and harness paths from defining different action precedence or reason codes. Protocol-neutral inputs keep AWS, Google, OpenAI, Anthropic, MCP, and Axum types out of policy code. A strict deterministic-first order avoids managed-service cost and latency after a local deny. Default fail-open behavior preserves availability while producing evidence that operators can use before selecting fail-closed behavior.

## Trade-Offs

Guarded streams add latency because the gateway cannot release partial output before it reconstructs and checks all generated tool calls. Configured byte and time limits bound this cost. Unguarded streams retain their current behavior.

Hash-only default records reduce forensic detail. Operators who need payloads must enable the existing capture policy and accept its redaction and retention controls.

Built-in packs cannot cover provider-specific tool names without aliases. Structured selectors therefore support documented aliases while retaining stable generic rule identities.
