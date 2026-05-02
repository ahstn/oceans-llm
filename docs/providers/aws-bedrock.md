# AWS Bedrock

`See also`: [Configuration Reference](../configuration/configuration-reference.md), [Model Routing and API Behavior](../configuration/model-routing-and-api-behavior.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md)

This page owns provider-specific configuration examples for Amazon Bedrock routes.

## Current Runtime Boundary

The gateway uses the Bedrock Runtime endpoint shape:

- non-Anthropic chat models use Bedrock `Converse`
- Anthropic Claude chat models use Bedrock `InvokeModel` with the native Anthropic Messages body for non-streaming requests
- streaming chat uses Bedrock `ConverseStream` and is normalized into OpenAI-compatible chat-completion chunks
- native Anthropic Messages streaming through `InvokeModelWithResponseStream` is not implemented in this gateway slice
- `/v1/responses` and `/v1/embeddings` are not implemented for `aws_bedrock` routes

The provider adapter supports bearer-token auth and IAM SigV4 request signing. AWS documents `AWS_BEARER_TOKEN_BEDROCK` as the environment variable recognized by Bedrock API-key auth and direct HTTP calls can pass the same value as `Authorization: Bearer ...`: [Use an Amazon Bedrock API key](https://docs.aws.amazon.com/en_us/bedrock/latest/userguide/api-keys-use.html).

For IAM auth, `auth.mode: default_chain` uses the AWS SDK default credential provider chain. In EKS, IRSA works through that chain when the pod environment includes `AWS_ROLE_ARN`, `AWS_WEB_IDENTITY_TOKEN_FILE`, and optional `AWS_ROLE_SESSION_NAME`; earlier sources in the default chain can still win, matching AWS SDK behavior. `auth.mode: static_credentials` signs with the configured access key, secret key, and optional session token.

## Provider

```yaml
providers:
  - id: bedrock-us-east-1
    type: aws_bedrock
    region: us-east-1
    auth:
      mode: bearer
      token: env.AWS_BEARER_TOKEN_BEDROCK
    display:
      label: Amazon Bedrock
      icon_key: aws
```

`endpoint_url` is optional. When omitted, the gateway uses `https://bedrock-runtime.{region}.amazonaws.com`.

IAM examples:

```yaml
providers:
  - id: bedrock-irsa
    type: aws_bedrock
    region: us-east-1
    auth:
      mode: default_chain

  - id: bedrock-static
    type: aws_bedrock
    region: us-east-1
    auth:
      mode: static_credentials
      access_key_id: env.AWS_ACCESS_KEY_ID
      secret_access_key: env.AWS_SECRET_ACCESS_KEY
      session_token: env.AWS_SESSION_TOKEN
```

## Model Identity

Use Bedrock Runtime model identities as `upstream_model` values. The value can be a base model ID, a geo or global inference profile ID, or a Bedrock ARN accepted by the Bedrock Runtime operation.

AWS now publishes model IDs on the Bedrock model cards. The [models at a glance](https://docs.aws.amazon.com/bedrock/latest/userguide/model-cards.html) page links to each model card and says those cards include programmatic model IDs, endpoint support, service tiers, regional availability, quotas, and sample code.

Examples verified against AWS model cards on 2026-04-30:

| Use case | Gateway model id | Bedrock `upstream_model` | Notes |
| --- | --- | --- | --- |
| Latest high-capability Claude | `claude-opus-bedrock` | `global.anthropic.claude-opus-4-7` | [Claude Opus 4.7](https://docs.aws.amazon.com/bedrock/latest/userguide/model-card-anthropic-claude-opus-4-7.html) launched on 2026-04-16. AWS lists `anthropic.claude-opus-4-7` plus US, EU, JP, and global inference IDs. |
| Claude regional profile | `claude-sonnet-bedrock` | `us.anthropic.claude-sonnet-4-5-20250929-v1:0` | [Claude Sonnet 4.5](https://docs.aws.amazon.com/bedrock/latest/userguide/model-card-anthropic-claude-sonnet-4-5.html) supports base, geo, and global IDs on Bedrock Runtime. |
| Amazon low-cost multimodal | `nova-lite-bedrock` | `global.amazon.nova-2-lite-v1:0` | [Nova 2 Lite](https://docs.aws.amazon.com/bedrock/latest/userguide/model-card-amazon-nova-2-lite.html) supports `amazon.nova-2-lite-v1:0`, US/EU geo IDs, and a global inference ID. |
| Amazon reasoning and distillation | `nova-premier-bedrock` | `us.amazon.nova-premier-v1:0` | [Nova Premier](https://docs.aws.amazon.com/bedrock/latest/userguide/model-card-amazon-nova-premier.html) supports `amazon.nova-premier-v1:0` and a US geo inference ID; AWS does not list a global inference ID for this model. |

Prefer geo or global inference profile IDs when your AWS account and residency policy allow them. They let Bedrock route within the selected geography or globally for higher throughput. Use in-region base model IDs when data residency requires one specific Region.

## Claude Example

Claude routes are detected by `upstream_model` values containing `anthropic.claude`. Non-streaming Chat Completions requests are sent to Bedrock Runtime `InvokeModel` with Anthropic Messages fields, including `anthropic_version: bedrock-2023-05-31`.

```yaml
models:
  - id: claude-opus-bedrock
    description: Claude Opus on AWS Bedrock
    tags: [bedrock, claude, reasoning]
    routes:
      - provider: bedrock-us-east-1
        upstream_model: global.anthropic.claude-opus-4-7
        capabilities:
          chat_completions: true
          responses: false
          embeddings: false
          stream: true
          tools: true
          vision: true
          json_schema: false
```

Native Claude invocation requires callers to set `max_tokens` or `max_completion_tokens`. Bedrock-hosted Claude vision is limited to base64 image payloads in this gateway slice; remote image URLs are rejected before the provider call.

### Claude Thinking Compatibility

For native Claude Messages invocation, OpenAI-shaped `reasoning_effort` maps to Anthropic Messages `output_config.effort` without forwarding the OpenAI-only field. On Claude Opus 4.7 and later, the gateway also sends `thinking: { "type": "adaptive" }` and rejects manual `thinking.type: "enabled"` budgets. On Claude Opus 4.6 and Claude Sonnet 4.6, `reasoning_effort` also selects adaptive thinking, while explicit manual `thinking` budgets remain pass-through for existing callers.

For Bedrock Converse and ConverseStream, the gateway uses Bedrock's provider-specific shape instead:

```json
{
  "additionalModelRequestFields": {
    "thinking": {
      "type": "adaptive",
      "effort": "high"
    }
  }
}
```

Older Claude models do not support adaptive thinking. Claude Sonnet 4.5, Claude Haiku 4.5, and earlier models require a manual budget via `thinking.budget_tokens`, `reasoning.budget_tokens`, `reasoning_budget_tokens`, or `thinking_budget_tokens`; the gateway sends `thinking: { "type": "enabled", "budget_tokens": ... }` and does not add `output_config.effort`. Claude Opus 4.5 additionally supports Bedrock's beta effort parameter for native Messages invocation, so the gateway adds `anthropic_beta: ["effort-2025-11-24"]` when it maps `reasoning_effort` to `output_config.effort` for that model.

For Opus 4.7 and later, non-default `temperature`, `top_p`, and `top_k` fail locally. Default `temperature: 1` and `top_p: 1` are omitted. If callers provide both normalized reasoning fields and provider-native `thinking` or `output_config` fields, the provider-native fields must agree with the normalized values; conflicting values are rejected. Other provider-native fields such as `anthropic_beta` and `context_management` remain pass-through.

### Streaming and Tool Continuations

Streaming Bedrock routes currently use Bedrock `ConverseStream`. Native Anthropic Messages streaming through `InvokeModelWithResponseStream` remains a separate follow-up tracked by [issue #139](https://github.com/ahstn/oceans-llm/issues/139). This matters for Claude-specific stream contracts: native Messages streams emit Anthropic SSE events such as `thinking_delta`, `signature_delta`, and `content_block_start`, while the current Bedrock stream path normalizes Bedrock Converse EventStream events.

Chat Completions hides Claude thinking from normal `content` and `delta.content`. Native Anthropic thinking blocks and Bedrock Converse reasoning content are preserved under `provider_metadata.aws_bedrock.reasoning` for debugging and provider continuity. The gateway does not yet rehydrate those preserved `thinking`, `signature`, or `redacted_thinking` blocks back into future request content when callers send tool results. Anthropic documents that tool-use continuations with thinking may require complete unmodified thinking blocks, so callers should treat this as unsupported gateway-managed continuity until [issue #140](https://github.com/ahstn/oceans-llm/issues/140) lands.

## Amazon Nova Example

Amazon Nova routes use the generic Bedrock Converse request shape.

```yaml
models:
  - id: nova-lite-bedrock
    description: Amazon Nova 2 Lite on AWS Bedrock
    tags: [bedrock, nova, multimodal]
    routes:
      - provider: bedrock-us-east-1
        upstream_model: global.amazon.nova-2-lite-v1:0
        capabilities:
          chat_completions: true
          responses: false
          embeddings: false
          stream: true
          tools: true
          vision: false
          json_schema: false

  - id: nova-premier-bedrock
    description: Amazon Nova Premier on AWS Bedrock
    tags: [bedrock, nova, reasoning]
    routes:
      - provider: bedrock-us-east-1
        upstream_model: us.amazon.nova-premier-v1:0
        capabilities:
          chat_completions: true
          responses: false
          embeddings: false
          stream: true
          tools: true
          vision: false
          json_schema: false
```

Keep `vision: false` unless the route has been tested with the exact public request shape you plan to support. Bedrock model cards can list multimodal support even when the gateway adapter has not normalized that modality for the public OpenAI-shaped request.

## Fallback Across Providers

Route priority and weight can put Bedrock behind another provider while keeping a stable gateway model id.

```yaml
models:
  - id: coding-default
    description: Stable model alias for coding workloads
    tags: [coding]
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
        priority: 10
        weight: 1.0
      - provider: bedrock-us-east-1
        upstream_model: global.anthropic.claude-opus-4-7
        priority: 20
        weight: 1.0
        capabilities:
          chat_completions: true
          responses: false
          embeddings: false
          stream: true
          tools: true
          vision: true
          json_schema: false
```

The current runtime executes one selected route. Priority and weight affect route planning, not live retry after an upstream error.

## Operational Notes

- Set `responses: false` and `embeddings: false` on Bedrock routes until those API families exist in the provider adapter.
- Keep `json_schema: false` unless a specific Bedrock route has explicit provider-specific overrides and tests.
- Use `extra_body` only for additive Bedrock or Anthropic fields you have tested for the exact model family.
- Chat Completions hides Claude thinking from normal `content` and `delta.content`. Native Anthropic thinking blocks and Bedrock Converse reasoning content are preserved under `provider_metadata.aws_bedrock.reasoning` for debugging and provider continuity. Exact reasoning/cache accounting remains tracked by [issue #92](https://github.com/ahstn/oceans-llm/issues/92), native Bedrock Anthropic streaming remains tracked by [issue #139](https://github.com/ahstn/oceans-llm/issues/139), and thinking block replay for tool-use continuations remains tracked by [issue #140](https://github.com/ahstn/oceans-llm/issues/140).
- Check the model card before adding a new `upstream_model`; Bedrock model IDs and inference profile support differ by model and Region.
- Prefer `default_chain` for production IAM roles and IRSA. Use `static_credentials` only for constrained local or controlled deployment cases where credential rotation is handled outside the gateway.

## Validation

Validate documentation-only edits with `mise run docs-check`. For runtime Bedrock adapter changes, run `mise run lint` and the focused provider tests such as `cargo test -p gateway-providers bedrock`.
