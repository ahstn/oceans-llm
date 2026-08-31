# Gateway Guardrails

Gateway guardrails let admins inspect and control model and tool traffic at the gateway. Policies can audit or deny operations with built-in deterministic checks, Amazon Bedrock Guardrails, or Google Cloud Model Armor.

Guardrails are configuration-authoritative. Callers cannot disable a policy, select a weaker policy, or supply an allow-once token.

`See also`: [Agent Harness Usage](agent-harness-usage.md), [AWS Bedrock](../providers/aws-bedrock.md), [Google Vertex AI](../providers/gcp-vertex.md), [Observability and Request Logs](observability-and-request-logs.md)

## How guardrails are applied

For each protected operation, the gateway resolves the default policy and then applies any matching model-route or MCP-server override. An override replaces only the fields it defines.

The gateway evaluates checks in this order:

1. Built-in deterministic packs run first. A deterministic deny stops evaluation.
2. Managed checks run once in the configured order.
3. Text transformed by an allowed managed check becomes the input to the next check and then to the protected operation.

The supported evaluation phases are:

| Phase | Protected boundary |
| --- | --- |
| `prompt` | Before the gateway invokes a model provider |
| `model_response` | Before a model response reaches the caller |
| `generated_tool_call` | After the gateway reconstructs a model-generated tool call and before it reaches the caller |
| `mcp_call` | Before the gateway looks up upstream credentials or sends an MCP request |
| `mcp_result` | Before an MCP result reaches the caller |
| `harness_pre_tool` | Before Pi or OpenCode starts a local shell process |

Guarded streams are buffered until the gateway reaches a final decision. The gateway releases no guarded bytes before that decision. Streams with no enabled policy retain their normal streaming behavior.

## Enable guardrails in audit mode

Guardrails are disabled when the `guardrails` section is absent or `default.enabled` is `false`. Start with `audit` mode so matching traffic is recorded without being blocked.

Add the following section to the gateway YAML configuration:

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
      - cloud.aws
      - cloud.gcp
      - saas.notion
    managed_checks: []
    stream_buffer_bytes: 4194304
    stream_buffer_timeout_ms: 120000
  managed_checks: {}
  model_routes: {}
  mcp_servers: {}
```

Restart the gateway with the updated configuration. Startup fails if the configuration references an unknown pack, managed check, model route, or MCP server. It also rejects unsupported phase combinations and invalid managed-service resource names.

The built-in pack IDs are versioned policy contracts:

| Pack | Traffic inspected |
| --- | --- |
| `core.shell` | Shell commands |
| `core.git` | Git operations |
| `core.filesystem` | Filesystem operations |
| `database.postgresql` | PostgreSQL operations |
| `cloud.aws` | AWS operations |
| `cloud.gcp` | Google Cloud operations |
| `saas.notion` | Notion MCP operations |

Shell checks parse command structure rather than matching raw command text. MCP checks use the server and tool identity, aliases, parsed JSON arguments, and typed JSON-path predicates. This prevents quoted text or serialized JSON formatting from changing how a rule is interpreted.

## Configure policy overrides

Use `model_routes` to change policy for a specific inference route. The key must use `{model}/{provider}/{upstream_model}` and match a configured route exactly.

Use `mcp_servers` to change policy for a registered MCP server. The key must match the configured server key.

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
    notion:
      mode: deny
      packs: [saas.notion]
```

In this example, most traffic remains in audit mode. The selected model route and Notion MCP server enforce denials. The model route inherits the default packs, while the MCP override replaces them with `saas.notion`.

An override can set any of these policy fields:

| Field | Purpose | Default policy value |
| --- | --- | --- |
| `enabled` | Enables evaluation for the scope | `false` |
| `mode` | Selects `audit` or `deny` behavior | `audit` |
| `packs` | Ordered list of built-in packs | `[]` |
| `managed_checks` | Ordered list of configured managed checks | `[]` |
| `stream_buffer_bytes` | Maximum buffered guarded response size | `4194304` |
| `stream_buffer_timeout_ms` | Maximum time to buffer an MCP result | `120000` |

`stream_buffer_timeout_ms` can be at most `600000`. If an upstream MCP tool keeps its event stream open beyond the configured timeout, the gateway returns `504` with a JSON-RPC guardrail error.

## Add Amazon Bedrock Guardrails

The Amazon Bedrock adapter calls the standalone Bedrock Runtime `ApplyGuardrail` API. It can protect traffic for any model provider because it is not coupled to an AWS Bedrock inference route. The referenced guardrail and version must already exist.

```yaml
guardrails:
  managed_checks:
    company-bedrock:
      kind: amazon_bedrock
      phases: [prompt, model_response, generated_tool_call, mcp_call, mcp_result]
      timeout_ms: 2000
      failure_disposition: fail_open
      max_content_bytes: 262144
      bedrock:
        region: us-east-1
        guardrail_identifier: a1b2c3d4e5f6
        guardrail_version: "1"
        auth:
          kind: default_chain
        max_retries: 2
  default:
    enabled: true
    mode: audit
    packs: [core.shell]
    managed_checks: [company-bedrock]
```

The AWS identity needs `bedrock:ApplyGuardrail` for the selected guardrail. `ApplyGuardrail` does not require model invocation permissions.

Use the default AWS credential chain in production. If a development environment requires static credentials, use `env.NAME` or `file./path` secret references. Do not put credential values in YAML.

A least-privilege identity policy has this shape:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": "bedrock:ApplyGuardrail",
      "Resource": "arn:aws:bedrock:REGION:ACCOUNT_ID:guardrail/GUARDRAIL_ID"
    }
  ]
}
```

For cross-account access, both the caller identity policy and the guardrail resource policy must allow `bedrock:ApplyGuardrail`. See [Set up permissions to use Amazon Bedrock Guardrails](https://docs.aws.amazon.com/bedrock/latest/userguide/guardrails-permissions.html) and [Using resource-based policies for guardrails](https://docs.aws.amazon.com/bedrock/latest/userguide/guardrails-resource-based-policies.html).

## Add Google Cloud Model Armor

The Model Armor adapter calls `sanitizeUserPrompt` for prompts and MCP calls. It calls `sanitizeModelResponse` for model responses and MCP results. The referenced templates must already exist.

```yaml
guardrails:
  managed_checks:
    company-model-armor:
      kind: google_model_armor
      phases: [prompt, model_response, mcp_call, mcp_result]
      timeout_ms: 2000
      failure_disposition: fail_open
      max_content_bytes: 262144
      model_armor:
        project: my-security-project
        location: us-central1
        prompt_template: projects/my-security-project/locations/us-central1/templates/prompt-policy
        response_template: projects/my-security-project/locations/us-central1/templates/response-policy
        auth:
          kind: bearer_token
          token: file./run/secrets/model-armor-token
  default:
    enabled: true
    mode: audit
    packs: [core.shell]
    managed_checks: [company-model-armor]
```

Grant the gateway service account the Model Armor User role:

```bash
gcloud projects add-iam-policy-binding PROJECT_ID \
  --member="serviceAccount:GATEWAY_SERVICE_ACCOUNT" \
  --role="roles/modelarmor.user"
```

For a custom role, grant `modelarmor.templates.useToSanitizeUserPrompt` and `modelarmor.templates.useToSanitizeModelResponse` on the referenced templates. The OAuth token must include the `https://www.googleapis.com/auth/cloud-platform` scope.

Use a protected `file./path` secret reference in production. The gateway reads the file before each evaluation, which allows an external credential process to rotate the token without restarting the gateway. An `env.NAME` reference is also supported, but its value remains fixed for the life of the gateway process.

See [Model Armor roles and permissions](https://docs.cloud.google.com/model-armor/access-control/roles-permissions) and the [`sanitizeUserPrompt` method](https://docs.cloud.google.com/model-armor/reference/rest/v1/projects.locations.templates/sanitizeUserPrompt).

## Choose managed failure behavior

Each managed check supports these fields:

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
5. Set one model-route or MCP-server override to `deny`.
6. Confirm expected traffic succeeds and denied operations create decision records.
7. Expand `deny` mode only after reviewing the observation window.

To roll back enforcement without losing decision history, change the affected override from `deny` to `audit` and restart the gateway. If evaluation causes unacceptable latency or protocol errors, set `enabled: false` on the affected override. Keep decision storage enabled while investigating.

## Investigate unexpected decisions

1. Record the decision ID and related request or MCP invocation ID. Do not copy sensitive payloads into an incident ticket.
2. Check the effective scope, phase, pack or managed service, reason code, failure disposition, and content hash.
3. For an unexpected deny, compare the configured policy with the resolved policy on the **Guardrails** page.
4. For repeated fail-open decisions, check cloud IAM, resource names, rate limits, timeouts, and regional service health.
5. Move the affected scope to `audit` if false denials block production traffic.

The guardrail architecture and composition rules are recorded in the [Gateway Guardrails ADR](https://github.com/ahstn/oceans-llm/blob/main/docs/adr/2026-08-22-gateway-guardrails-domain-and-composition.md). The implementation was introduced in [pull request #305](https://github.com/ahstn/oceans-llm/pull/305).
