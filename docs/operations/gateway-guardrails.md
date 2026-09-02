# Gateway Guardrails

Gateway guardrails let admins inspect and control model and tool traffic at the gateway. Policies can audit or deny operations with [built-in deterministic packs](guardrails/built-in-packs.md), [Amazon Bedrock Guardrails](guardrails/amazon-bedrock.md), or [Google Cloud Model Armor](guardrails/google-model-armor.md).

Guardrails are configuration-authoritative. Callers cannot disable a policy, select a weaker policy, or supply an allow-once token.

`See also`: [Agent Harness Usage](agent-harness-usage.md), [Observability and Request Logs](observability-and-request-logs.md), [Built-in Deterministic Packs](guardrails/built-in-packs.md), [Amazon Bedrock Guardrails](guardrails/amazon-bedrock.md), [Google Cloud Model Armor](guardrails/google-model-armor.md)

## Choose a guardrail type

| Guardrail type | Best suited to | Execution |
| --- | --- | --- |
| Built-in deterministic packs | Destructive commands, generated tool calls, and structured MCP operations | Runs locally without a managed-service dependency |
| Amazon Bedrock Guardrails | Applying an existing Bedrock content policy to model and MCP traffic | Calls the standalone Bedrock Runtime `ApplyGuardrail` API |
| Google Cloud Model Armor | Sanitizing prompts, model responses, MCP calls, and MCP results with existing templates | Calls the Model Armor prompt or response sanitization API |

Built-in packs and managed checks can be used together. Deterministic packs run first and stop evaluation when they deny an operation. Managed checks then run once in their configured order.

## How guardrails are applied

For each protected operation, the gateway resolves the default policy and then applies any matching model-route or MCP-server override. An override replaces only the fields it defines.

The gateway evaluates checks in this order:

1. Built-in deterministic packs run in their configured order.
2. A deterministic deny stops evaluation before any managed request is made.
3. Managed checks run once in their configured order.
4. Text transformed by an allowed managed check becomes the input to the next check and then to the protected operation.

The supported phases are:

| Phase | Protected boundary |
| --- | --- |
| `prompt` | Before the gateway invokes a model provider |
| `model_response` | Before a model response reaches the caller |
| `generated_tool_call` | After reconstruction of a model-generated tool call and before caller release |
| `mcp_call` | Before upstream credential lookup or MCP network I/O |
| `mcp_result` | Before an MCP result reaches the caller |
| `harness_pre_tool` | Before Pi or OpenCode starts a local shell process |

Guarded streams are buffered until the gateway reaches a final decision. The gateway releases no guarded bytes before that decision. Streams with no enabled policy retain their normal streaming behavior.

## Enable guardrails in audit mode

Guardrails are disabled when the `guardrails` section is absent or `default.enabled` is `false`. Start with `audit` mode so matches are recorded without blocking operations.

```yaml
guardrails:
  default:
    enabled: true
    mode: audit
    packs:
      - core.shell
      - core.git
      - core.filesystem
      - database.postgresql
      - database.snowflake
      - secrets.aws_secrets
      - secrets.onepassword
      - cloud.aws
      - cloud.gcp
      - kubernetes.kubectl
      - kubernetes.helm
      - saas.github
      - saas.notion
    managed_checks: []
    stream_buffer_bytes: 4194304
    stream_buffer_timeout_ms: 120000
  managed_checks: {}
  model_routes: {}
  mcp_servers: {}
```

This matches the mutation-focused packs enabled by the checked-in development and production configurations. The `secret_disclosure` pack is deliberately excluded because it blocks commands that print secret values. Review its rollout separately in [Built-in Deterministic Packs](guardrails/built-in-packs.md#protect-secret-values).

Restart the gateway after changing its YAML configuration. Startup fails if the configuration references an unknown pack, managed check, model route, or MCP server. It also rejects unsupported phase combinations and invalid managed-service resource names.

## Configure policy overrides

Use `model_routes` to change policy for a specific inference route. Its key must use `{model}/{provider}/{upstream_model}` and match a configured route exactly.

Use `mcp_servers` to change policy for a registered MCP server. Its key must match the configured server key.

```yaml
guardrails:
  default:
    enabled: true
    mode: audit
    packs: [core.shell, core.git]
    managed_checks: []
  model_routes:
    openai-fast/openai-prod/gpt-5:
      mode: deny
  mcp_servers:
    github:
      mode: deny
      packs: [saas.github]
    notion:
      mode: deny
      packs: [saas.notion]
```

In this example, most traffic remains in audit mode. The selected model route and MCP servers enforce denials. The model route inherits the default packs, while each MCP override replaces the pack list.

An override can set these policy fields:

| Field | Purpose | Default policy value |
| --- | --- | --- |
| `enabled` | Enables evaluation for the scope | `false` |
| `mode` | Selects `audit` or `deny` behavior | `audit` |
| `packs` | Ordered list of built-in packs | `[]` |
| `managed_checks` | Ordered list of configured managed checks | `[]` |
| `stream_buffer_bytes` | Maximum buffered guarded response size | `4194304` |
| `stream_buffer_timeout_ms` | Maximum time to buffer an MCP result | `120000` |

`stream_buffer_timeout_ms` can be at most `600000`. If an upstream MCP tool keeps its event stream open beyond the configured timeout, the gateway returns `504` with a JSON-RPC guardrail error.

## Choose managed failure behavior

Each managed check supports these common fields:

| Field | Meaning | Default |
| --- | --- | --- |
| `phases` | Gateway phases evaluated by the check | Required |
| `timeout_ms` | Request timeout from `1` to `120000` milliseconds | `2000` |
| `failure_disposition` | `fail_open` or `fail_closed` | `fail_open` |
| `max_content_bytes` | Maximum content sent to the service | `262144` |

With `fail_open`, a timeout, unavailable service, rate limit, malformed response, or content-size failure permits the operation and records an audit decision. With `fail_closed`, the same managed-service failure becomes a deny.

Keep `fail_open` during the initial rollout. Use `fail_closed` only when the security requirement outweighs the availability impact and the managed service has met its availability target.

Managed services may return masked or sanitized text. An allowed transformation is passed to the next check and then to the provider or caller. The gateway rejects transformations that cannot preserve the protocol payload shape.

## Verify decisions

Platform admins can open **Observability > Guardrails** in the admin control plane. The page shows resolved policies and privacy-safe decision metadata. Admin API clients can use:

- `GET /api/v1/admin/guardrails/policies`
- `GET /api/v1/admin/guardrails/decisions`

Decision records include the decision ID, request or MCP invocation context, phase, effective scope, evaluator, managed service, pack and rule ID, action, reason code, latency, failure disposition, transformation status, and a SHA-256 content hash.

By default, decision records do not contain prompts, commands, JSON arguments, or results. Raw payload retention follows the existing opt-in payload capture, redaction, and retention configuration.

Guardrail metrics are emitted as `gateway.guardrails.decisions` and `gateway.guardrails.decision.duration`. Monitor sustained fail-open decisions, changes in deny rate, latency near managed-check timeouts, and buffer-limit failures.

## Move from audit to deny

Roll out enforcement one scope at a time:

1. Enable the required packs and managed checks in `audit` mode.
2. Send representative inference, MCP, Pi, and OpenCode traffic through the gateway.
3. Review matches, fail-open events, managed latency, and transformations on the **Guardrails** page.
4. Correct policy scopes, aliases, phase selection, limits, and managed-service permissions.
5. Run `mise run guardrail-release-gate`. It runs the guardrail security, load, harness, and end-to-end gates against the production gateway binary with fake managed-service endpoints. Do not move a scope to `deny` while the gate fails.
6. Set one model-route or MCP-server override to `deny`.
7. Confirm expected traffic succeeds and denied operations create decision records.
8. Expand `deny` mode only after reviewing the observation window.

To roll back enforcement without losing decision history, change the affected override from `deny` to `audit` and restart the gateway. If evaluation causes unacceptable latency or protocol errors, set `enabled: false` on the affected override. Keep decision storage enabled while investigating.

## Investigate unexpected decisions

1. Record the decision ID and related request or MCP invocation ID. Do not copy sensitive payloads into an incident ticket.
2. Check the effective scope, phase, pack or managed service, reason code, failure disposition, and content hash.
3. For an unexpected deny, compare the configured policy with the resolved policy on the **Guardrails** page.
4. For repeated fail-open decisions, check cloud IAM, resource names, rate limits, timeouts, and regional service health.
5. Move the affected scope to `audit` if false denials block production traffic.

The guardrail architecture and composition rules are recorded in the [Gateway Guardrails ADR](https://github.com/ahstn/oceans-llm/blob/main/docs/adr/2026-08-22-gateway-guardrails-domain-and-composition.md). Guardrails were introduced in [pull request #305](https://github.com/ahstn/oceans-llm/pull/305), and the deterministic catalog was expanded in [pull request #317](https://github.com/ahstn/oceans-llm/pull/317).
