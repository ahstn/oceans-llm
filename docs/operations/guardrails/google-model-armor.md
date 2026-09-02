# Google Cloud Model Armor

Oceans can use existing Google Cloud Model Armor templates to sanitize model and MCP traffic. The adapter calls `sanitizeUserPrompt` for input boundaries and `sanitizeModelResponse` for output boundaries.

`See also`: [Gateway Guardrails](../gateway-guardrails.md), [Built-in Deterministic Packs](built-in-packs.md), [Google Vertex AI](../../providers/gcp-vertex.md)

## Prerequisites

Before configuring the managed check:

- Create the required Model Armor templates outside Oceans.
- Record the Google Cloud project, location, and full template resource names.
- Grant the gateway service account permission to use each template.
- Obtain an OAuth access token with the `https://www.googleapis.com/auth/cloud-platform` scope.
- Store the token in a protected file or environment variable.

Oceans references Model Armor templates. It does not create, update, or delete them. The protected inference route does not need to use Google Vertex AI as its model provider.

## Configure the managed check

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

Replace the project, location, and template resource names. The name `company-model-armor` is local to the Oceans configuration and is referenced from the ordered `managed_checks` list.

Restart the gateway after changing its configuration. Startup validates the phase and template combination, resource names, timeout, content limit, and provider-specific settings.

## Choose phases and templates

Model Armor supports all six phases:

| Phase | Model Armor operation | Required template |
| --- | --- | --- |
| `prompt` | `sanitizeUserPrompt` | `prompt_template` |
| `mcp_call` | `sanitizeUserPrompt` | `prompt_template` |
| `harness_pre_tool` | `sanitizeUserPrompt` | `prompt_template` |
| `model_response` | `sanitizeModelResponse` | `response_template` |
| `generated_tool_call` | `sanitizeModelResponse` | `response_template` |
| `mcp_result` | `sanitizeModelResponse` | `response_template` |

Configure a prompt template when the check includes an input-side phase and a response template when it includes an output-side phase. Startup validation rejects a phase whose template is missing. Model Armor evaluates text, so `harness_pre_tool` and `generated_tool_call` send the serialized command or tool call. Use [built-in deterministic packs](built-in-packs.md) alongside a managed check when you need structural matching for those phases.

## Grant Google Cloud permission

Grant the predefined Model Armor User role to the gateway service account:

```bash
gcloud projects add-iam-policy-binding PROJECT_ID \
  --member="serviceAccount:GATEWAY_SERVICE_ACCOUNT" \
  --role="roles/modelarmor.user"
```

For a least-privilege custom role, grant these permissions on the referenced templates:

- `modelarmor.templates.useToSanitizeUserPrompt`
- `modelarmor.templates.useToSanitizeModelResponse`

Only the permission corresponding to each configured operation is required. See [Model Armor roles and permissions](https://docs.cloud.google.com/model-armor/access-control/roles-permissions) and the [`sanitizeUserPrompt` method](https://docs.cloud.google.com/model-armor/reference/rest/v1/projects.locations.templates/sanitizeUserPrompt).

## Configure token rotation

Use a protected file reference in production:

```yaml
model_armor:
  project: my-security-project
  location: us-central1
  prompt_template: projects/my-security-project/locations/us-central1/templates/prompt-policy
  response_template: projects/my-security-project/locations/us-central1/templates/response-policy
  auth:
    kind: bearer_token
    token: file./run/secrets/model-armor-token
```

The gateway reads the file before each evaluation. An external credential process can replace the token without restarting the gateway.

An `env.NAME` secret reference is also supported, but its value remains fixed for the life of the gateway process. Restart the gateway after rotating an environment-backed token. Do not store the token value directly in YAML.

## Configure failure behavior

`failure_disposition` defaults to `fail_open`. A timeout, unavailable service, rate limit, malformed response, or content-size failure then permits the operation and records an audit decision.

Set `fail_closed` only after the service, IAM, and token-rotation path have met their availability target. With `fail_closed`, a managed-service failure becomes a deny and can stop otherwise valid model or MCP traffic.

`max_content_bytes` defaults to `262144`. Content above the limit follows the configured failure disposition rather than being sent to Model Armor.

## Handle sanitization

Model Armor can allow content unchanged, intervene, or return sanitized text. Oceans passes an allowed transformation to the next configured check and then to the protected provider or caller.

The gateway rejects a transformation that cannot preserve the protocol payload shape. Verify sanitization separately for prompts, model responses, MCP calls, and MCP results because each boundary has a different payload structure.

## Verify the integration

1. Keep the policy in `audit` and `fail_open` during initial verification.
2. Send representative allowed content through every configured phase.
3. Send privacy-safe fixture content that the selected template sanitizes or intervenes on.
4. Open **Observability > Guardrails** and filter for the managed check.
5. Confirm the decision records show the expected phase, action, managed service, latency, failure disposition, and transformation status.
6. Confirm sanitized content reaches the next check and protected boundary in protocol-valid form.
7. Rotate the token file and confirm subsequent evaluations continue without a gateway restart.
8. Review fail-open counts and latency before enabling `deny` or `fail_closed`.

Oceans records privacy-safe decision metadata by default. It does not place raw prompts, commands, MCP arguments, or results in the guardrail decision record.

## Troubleshoot failures

For repeated fail-open or fail-closed decisions, check:

- The project and location match each template resource name.
- The required prompt or response template is configured for every selected phase.
- The service account has the required template permission.
- The OAuth token is current and includes the cloud-platform scope.
- The process can read the configured token file.
- The configured timeout accommodates observed service latency.
- Content remains below `max_content_bytes`.
- Regional Model Armor service health and project quotas are normal.

Return an enforcing scope to `audit` if unexpected denials affect production traffic. Keep decision records enabled so admins can correlate the incident without retaining raw sensitive content.
