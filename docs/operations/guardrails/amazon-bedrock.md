# Amazon Bedrock Guardrails

Oceans can apply an existing Amazon Bedrock Guardrail to model and MCP traffic. The adapter calls the standalone Bedrock Runtime `ApplyGuardrail` API, so the protected inference route does not need to use Amazon Bedrock as its model provider.

`See also`: [Gateway Guardrails](../gateway-guardrails.md), [Built-in Deterministic Packs](built-in-packs.md), [AWS Bedrock](../../providers/aws-bedrock.md)

## Prerequisites

Before configuring the managed check:

- Create and publish the Bedrock guardrail outside Oceans.
- Record its Region, identifier, and version.
- Give the gateway AWS identity `bedrock:ApplyGuardrail` permission for that resource.
- Make AWS credentials available through the default credential chain or supported secret references.
- Decide which gateway phases should send content to the guardrail.

Oceans references the guardrail. It does not create, update, version, or delete Bedrock guardrail resources.

## Configure the managed check

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

Replace the example Region, identifier, and version with the published guardrail. The name `company-bedrock` is local to the Oceans configuration and is referenced from the ordered `managed_checks` list.

Restart the gateway after changing its configuration. Startup validates the phase list, resource identifiers, timeout, content limit, and provider-specific settings.

## Choose phases

The Bedrock adapter supports all six phases. Input-side phases are sent as `INPUT` content and output-side phases as `OUTPUT` content:

| Phase | Bedrock content source |
| --- | --- |
| `prompt` | `INPUT` |
| `mcp_call` | `INPUT` |
| `harness_pre_tool` | `INPUT` |
| `model_response` | `OUTPUT` |
| `generated_tool_call` | `OUTPUT` |
| `mcp_result` | `OUTPUT` |

Choose only the phases covered by the Bedrock policy and required by the protected workflow. Bedrock evaluates text, so `harness_pre_tool` and `generated_tool_call` send the serialized command or tool call. Use [built-in deterministic packs](built-in-packs.md) alongside a managed check when you need structural matching for those phases.

## Grant AWS permission

The gateway identity needs only `bedrock:ApplyGuardrail` for the selected guardrail. The standalone API does not require model invocation permission.

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

Replace `REGION`, `ACCOUNT_ID`, and `GUARDRAIL_ID`. For cross-account access, both the caller identity policy and the guardrail resource policy must allow `bedrock:ApplyGuardrail`.

See [Set up permissions to use Amazon Bedrock Guardrails](https://docs.aws.amazon.com/bedrock/latest/userguide/guardrails-permissions.html) and [Using resource-based policies for guardrails](https://docs.aws.amazon.com/bedrock/latest/userguide/guardrails-resource-based-policies.html).

## Configure authentication

Use the default AWS credential chain in production:

```yaml
bedrock:
  region: us-east-1
  guardrail_identifier: a1b2c3d4e5f6
  guardrail_version: "1"
  auth:
    kind: default_chain
```

If a development fixture requires static credentials, configure `env.NAME` or `file./path` secret references for the access key, secret key, and optional session token. Do not put credential values directly in YAML.

The configured Region is the Region used for `ApplyGuardrail`. It is independent of any model provider Region.

## Configure failure behavior

`failure_disposition` defaults to `fail_open`. A timeout, unavailable service, rate limit, malformed response, or content-size failure then permits the operation and records an audit decision.

Set `fail_closed` only after the service and IAM path have met their availability target. With `fail_closed`, a managed-service failure becomes a deny and can stop otherwise valid model or MCP traffic.

`max_content_bytes` defaults to `262144`. Content above the limit follows the configured failure disposition rather than being sent to Bedrock.

## Verify the integration

1. Keep the policy in `audit` and `fail_open` during initial verification.
2. Send representative allowed content through every configured phase.
3. Send privacy-safe fixture content that the Bedrock guardrail intervenes on.
4. Open **Observability > Guardrails** and filter for the managed check.
5. Confirm the decision records show the expected phase, action, managed service, latency, and failure disposition.
6. Confirm allowed masking or transformation preserves the model or MCP protocol shape.
7. Review fail-open counts and latency before enabling `deny` or `fail_closed`.

Oceans records privacy-safe decision metadata by default. It does not place raw prompts, commands, MCP arguments, or results in the guardrail decision record.

## Troubleshoot failures

For repeated fail-open or fail-closed decisions, check:

- The guardrail identifier and version exist in the configured Region.
- The runtime AWS identity resolves as expected.
- `bedrock:ApplyGuardrail` is allowed by identity and resource policies.
- Cross-account policy conditions permit the caller.
- The configured timeout accommodates observed service latency.
- Content remains below `max_content_bytes`.
- Regional Bedrock service health and account quotas are normal.

Return an enforcing scope to `audit` if unexpected denials affect production traffic. Keep decision records enabled so admins can correlate the incident without retaining raw sensitive content.
