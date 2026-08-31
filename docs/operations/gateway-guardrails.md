# Gateway Guardrails

`See also`: [Agent Harness Usage](agent-harness-usage.md), [AWS Bedrock](../providers/aws-bedrock.md), [Google Vertex AI](../providers/gcp-vertex.md), [Observability and Request Logs](observability-and-request-logs.md)

Gateway guardrails inspect model and tool traffic before a protected operation crosses its execution or release boundary. Configuration is the only policy authority. Callers cannot bypass a policy, supply an allow-once token, or select a weaker scope.

## Evaluation order

For each configured phase, the gateway resolves the global policy and then applies a model-route or MCP-server override. It runs built-in deterministic packs first. A deterministic deny stops the chain. If deterministic checks allow the operation, managed checks run once in the configured order. An allowed transformation becomes the input to the next check and to the protected operation.

Supported phases are:

- `prompt`: before provider invocation.
- `model_response`: before a model response reaches the caller.
- `generated_tool_call`: after complete tool-call reconstruction and before caller release.
- `mcp_call`: before upstream credential lookup or network I/O.
- `mcp_result`: before an MCP result reaches the caller.
- `harness_pre_tool`: before Pi or OpenCode starts a local shell process.

A guarded stream is buffered up to `stream_buffer_bytes`. The gateway releases no guarded bytes before the final decision. Streams for disabled policies keep their normal streaming behavior.

A guarded MCP result is buffered for at most `stream_buffer_timeout_ms` (default `120000`, maximum `600000`). An upstream MCP tool that holds its event stream open past that bound is answered with `504` and a JSON-RPC guardrail error instead of holding the caller connection open.

## Built-in packs

The versioned built-in pack IDs are `core.shell`, `core.git`, `core.filesystem`, `database.postgresql`, `database.snowflake`, `cloud.aws`, `cloud.gcp`, `kubernetes.kubectl`, `kubernetes.helm`, `secrets.aws_secrets`, `secrets.onepassword`, `secret_disclosure`, and `saas.notion`. Shell checks parse command structure. MCP checks use the server identity, tool identity and aliases, parsed JSON arguments, and typed JSON-path predicates. They do not match a regular expression against serialized JSON.

`secret_disclosure` is opt-in. It blocks secret-manager read commands that expose values to model-visible output. The provider-specific secret packs prevent destructive mutation and can remain in the default policy.

## Audit-first configuration

Start with `audit`. Audit mode records matches but does not stop the operation.

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
      - cloud.aws
      - cloud.gcp
      - kubernetes.kubectl
      - kubernetes.helm
      - secrets.aws_secrets
      - secrets.onepassword
      - saas.notion
    managed_checks: []
    stream_buffer_bytes: 4194304
    stream_buffer_timeout_ms: 120000
  managed_checks: {}
  model_routes: {}
  mcp_servers: {}
```

A model-route key has the form `{model}/{provider}/{upstream_model}`. An MCP key is the configured server key. An override replaces only the fields it defines.

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

Startup rejects unknown packs, managed checks, model routes, MCP servers, invalid phase combinations, and invalid managed resource names.

## Amazon Bedrock Guardrails

The adapter uses the standalone Bedrock Runtime `ApplyGuardrail` API. It is independent of the route's model provider. It only references a guardrail that already exists.

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

The AWS identity needs `bedrock:ApplyGuardrail` for the selected guardrail. Use the default AWS credential chain in production. If static credentials are required for a development fixture, store them as `env.NAME` or `file./path` secret references. Do not put credentials in YAML.

Use a least-privilege identity policy. Replace the region, account, and guardrail ID placeholders. `ApplyGuardrail` does not need model invocation permissions because Oceans calls the standalone API.

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

For cross-account use, the caller identity policy and the guardrail resource policy must both allow `bedrock:ApplyGuardrail`. See [Set up permissions to use Amazon Bedrock Guardrails](https://docs.aws.amazon.com/bedrock/latest/userguide/guardrails-permissions.html) and [Using resource-based policies for guardrails](https://docs.aws.amazon.com/bedrock/latest/userguide/guardrails-resource-based-policies.html).

## Google Cloud Model Armor

The adapter calls `sanitizeUserPrompt` for prompt and MCP-call input and `sanitizeModelResponse` for model responses and MCP results. It is independent of the route's model provider. Templates must already exist.

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
          token: env.MODEL_ARMOR_ACCESS_TOKEN
  default:
    enabled: true
    mode: audit
    packs: [core.shell]
    managed_checks: [company-model-armor]
```

The Google identity needs `modelarmor.templates.useToSanitizeUserPrompt` or `modelarmor.templates.useToSanitizeModelResponse` on each referenced template. For production, write a current OAuth access token to a protected file and configure a `file./path` secret reference. The gateway reads that file before each evaluation, so an external credential process can rotate the token without restarting the gateway. An `env.NAME` reference is also supported, but its value is fixed for the life of the process. Do not store the token in YAML.

Grant the predefined Model Armor User role to the gateway service account:

```bash
gcloud projects add-iam-policy-binding PROJECT_ID \
  --member="serviceAccount:GATEWAY_SERVICE_ACCOUNT" \
  --role="roles/modelarmor.user"
```

For a least-privilege custom role, include `modelarmor.templates.useToSanitizeUserPrompt` and `modelarmor.templates.useToSanitizeModelResponse`. The access token must include the `https://www.googleapis.com/auth/cloud-platform` scope. See [Model Armor roles and permissions](https://docs.cloud.google.com/model-armor/access-control/roles-permissions) and the [`sanitizeUserPrompt` method](https://docs.cloud.google.com/model-armor/reference/rest/v1/projects.locations.templates/sanitizeUserPrompt).

## Failures and transformations

`failure_disposition` defaults to `fail_open`. A timeout, unavailable service, rate limit, malformed response, or content-size failure then permits the operation and records an audit decision. Set `fail_closed` only after the managed service has met its availability target. Fail-closed converts the managed failure to a deny.

Allowed masked or sanitized text is passed to the next check and then to the caller or upstream service. The gateway rejects a transformation that cannot preserve the protocol payload shape.

## Observability

The platform-admin UI has a read-only **Guardrails** page. The API endpoints are:

- `GET /api/v1/admin/guardrails/policies`
- `GET /api/v1/admin/guardrails/decisions`

Decision events include the decision ID, related request ID or MCP invocation context, phase, effective scope, evaluator, managed service, pack and rule ID, action, stable reason code, latency, failure disposition, transformation flag, and SHA-256 content hash. Default events do not contain prompts, commands, JSON arguments, or results. Payload retention follows the existing opt-in payload capture, redaction, and retention policy.

Metrics use `gateway.guardrails.decisions` and `gateway.guardrails.decision.duration`. Alert on sustained fail-open decisions, deny-rate changes, latency near the configured timeout, and buffer-limit failures.

## Release gate

Run `mise run guardrail-release-gate` before an audit-to-deny change. The gate uses the production gateway binary, LibSQL migrations, the admin UI build, fake managed-service endpoints, and the first-party harness hook artifacts.

| Boundary | Safe or transformed case | Denied case and side-effect proof |
| --- | --- | --- |
| OpenAI Chat Completions, Anthropic Messages, and OpenAI Responses prompts | Audit-mode managed intervention reaches the provider; an allowed mask reaches the provider in protocol-valid form | Deny mode returns the stable policy error and the mock provider request count remains zero |
| Non-stream model response | Safe text reaches the caller | A destructive generated tool call returns the stable policy error without executable arguments |
| Stream model response | Safe SSE is released after the final decision | Split, parallel, malformed, and destructive tool-call fixtures release no guarded stream bytes; an oversize stream returns `413` with a bounded JSON error |
| Direct MCP call and result | Safe calls execute; managed result masking preserves JSON | Deterministic and managed call denies leave the upstream execution count at zero |
| Aggregate MCP call and result | Safe calls execute; managed result masking preserves the aggregate response | The aggregate policy-denied JSON-RPC result leaves the upstream execution count at zero |
| Pi and OpenCode | Allow and audit decisions retain the decision ID | The pre-tool hook rejects before the shell implementation or process handoff runs |
| Managed failures and observability | Fake Model Armor masking is visible in the protected output | A fake managed failure creates a `fail_open` audit event in structured logs, guardrail metrics, persistence, the admin API, and the read-only UI |

The load budgets are release gates, not sizing promises for every deployment:

- Deterministic evaluation: 10,000 local pack evaluations must finish within two seconds.
- Managed concurrency: 64 concurrent 20 ms managed evaluations must finish within two seconds. Each managed call still has its configured `timeout_ms`; the release fixture uses 1,000 ms.
- Managed request body: each check rejects content above `max_content_bytes`; the documented default is 256 KiB.
- Guarded response memory: each stream buffers at most `stream_buffer_bytes`; the release stack uses 4 MiB and verifies that overflow releases only a small JSON error.
- MCP request body: the gateway rejects a body above 4 MiB before dispatch.
- Event persistence: 100 privacy-safe LibSQL decision records must insert and list within five seconds. PostgreSQL runs the same metadata, filter, ordering, and pagination contract when `TEST_POSTGRES_URL` is set.

Release checklist:

1. Run `mise run guardrail-release-gate`.
2. Run the repository checks: `mise run rust-fmt-check`, `mise run rust-lint`, `mise run rust-test`, `mise run admin-contract-check`, `mise run harness-integration-typecheck`, `mise run test`, `mise run lint`, `mise run build`, and `mise run //docs:build`.
3. Confirm every configured inference route, direct MCP route, aggregate MCP route, and first-party harness appears in the matrix. Do not approve a new route without a deny-before-side-effect fixture.
4. Confirm default decision records, logs, API responses, and UI output contain hashes and identifiers but no raw prompt, command, argument, result, credential, or managed-service token.
5. Confirm the production-shaped local stack lets the `audit-fast` route reach the provider for the same managed intervention that the `fast` deny route blocks.
6. Review fail-open counts and managed latency before changing a production scope from `audit` to `deny`.

## Rollout and rollback

1. Enable all required packs in `audit` mode.
2. Run representative prompt, response, direct MCP, aggregate MCP, Pi, and OpenCode traffic.
3. Review near misses, fail-open events, managed latency, and transformation validity in the admin view.
4. Correct aliases, phases, limits, or managed resource permissions in configuration.
5. Change one route or MCP server to `deny` mode.
6. Confirm that denied MCP calls make zero upstream requests, denied harness calls make zero local processes, and guarded streams release no early bytes.
7. Expand deny mode by route or server after the observation window.

To roll back enforcement without losing evidence, change the affected override from `deny` to `audit` and restart the gateway. If guardrails cause unacceptable latency or protocol errors, set `enabled: false` for the affected override. Do not remove event storage during an incident.

## Incident handling

1. Record the decision ID and related request or MCP invocation ID. Do not copy raw sensitive content into the incident ticket.
2. Check the effective scope, phase, pack or managed service, reason code, failure disposition, and content hash.
3. For an unexpected deny, reproduce with privacy-safe fixtures and compare the resolved policy with the read-only admin view.
4. For a fail-open spike, check cloud IAM, resource names, rate limits, timeouts, and regional service health.
5. Move the affected scope to `audit` if false denials block production. Use `fail_closed` only when the security risk of unavailable checks is greater than the availability impact.
6. Preserve decision metadata and metrics for the incident timeline. Retain raw payloads only under the existing approved capture policy.
