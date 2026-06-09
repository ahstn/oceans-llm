# OpenAI GPT-5.5 on Bedrock Mantle

`See also`: [AWS Bedrock](aws-bedrock.md), [Configuration Reference](../configuration/configuration-reference.md), [Provider API Compatibility](../reference/provider-api-compatibility.md)

This page shows how to configure OpenAI GPT-5.5 through Amazon Bedrock Mantle. It is for admins configuring gateway routes for users.

AWS lists GPT-5.5 with model ID `openai.gpt-5.5`, launch Region `us-east-2`, endpoint kind `bedrock-mantle`, and Responses path `https://bedrock-mantle.{region}.api.aws/openai/v1/responses`: [AWS model card](https://docs.aws.amazon.com/bedrock/latest/userguide/model-card-openai-gpt-55.html). The AWS launch post and OpenAI Bedrock guide use the same Mantle base URL shape: [AWS launch post](https://aws.amazon.com/blogs/aws/get-started-with-openai-gpt-5-5-gpt-5-4-models-and-codex-on-amazon-bedrock/) and [OpenAI Amazon Bedrock guide](https://developers.openai.com/api/docs/guides/amazon-bedrock).

## Bedrock API Key

Set `AWS_BEARER_TOKEN_BEDROCK` in the gateway runtime environment and configure bearer auth:

```yaml
providers:
  - id: bedrock-mantle-openai
    type: aws_bedrock
    region: us-east-2
    endpoint_kind: bedrock_mantle
    auth:
      mode: bearer
      token: env.AWS_BEARER_TOKEN_BEDROCK
    display:
      label: Bedrock Mantle OpenAI
      icon_key: aws

models:
  - id: gpt-55-bedrock
    description: OpenAI GPT-5.5 on Amazon Bedrock Mantle
    tags: [bedrock, mantle, openai, gpt-5-5]
    routes:
      - provider: bedrock-mantle-openai
        upstream_model: openai.gpt-5.5
        capabilities:
          chat_completions: false
          responses: true
          stream: true
          embeddings: false
          tools: true
          vision: true
          json_schema: true
        compatibility:
          aws_bedrock:
            api_style: mantle_openai_responses
            openai_base_path: /openai/v1
```

For OpenAI-shaped Mantle routes, the gateway sends API-key auth as `Authorization: Bearer ...`.

## AWS SigV4 Provider Chain

Use `auth.mode: default_chain` when the gateway should sign Mantle requests with the AWS SDK default credential provider chain:

```yaml
providers:
  - id: bedrock-mantle-openai
    type: aws_bedrock
    region: us-east-2
    endpoint_kind: bedrock_mantle
    auth:
      mode: default_chain

models:
  - id: gpt-55-bedrock
    routes:
      - provider: bedrock-mantle-openai
        upstream_model: openai.gpt-5.5
        capabilities:
          chat_completions: false
          responses: true
          stream: true
          embeddings: false
        compatibility:
          aws_bedrock:
            api_style: mantle_openai_responses
            openai_base_path: /openai/v1
```

For `endpoint_kind: bedrock_mantle`, SigV4 uses service name `bedrock-mantle`. For Runtime providers, SigV4 uses service name `bedrock`.

## Bedrock Projects

Bedrock Projects for OpenAI-compatible APIs use the `OpenAI-Project` request header: [AWS Projects](https://docs.aws.amazon.com/bedrock/latest/userguide/projects.html). Configure it statically per route with `extra_headers`:

```yaml
models:
  - id: gpt-55-bedrock
    routes:
      - provider: bedrock-mantle-openai
        upstream_model: openai.gpt-5.5
        capabilities:
          chat_completions: false
          responses: true
          stream: true
          embeddings: false
          tools: true
          vision: true
          json_schema: true
        extra_headers:
          OpenAI-Project: proj_123
        compatibility:
          aws_bedrock:
            api_style: mantle_openai_responses
            openai_base_path: /openai/v1
```

Projects apply to OpenAI-compatible Mantle routes. Do not rely on caller-supplied inbound headers for project routing; configure `OpenAI-Project` in the selected route.

## Caller Usage

Call the gateway's `/v1/responses` endpoint with the configured gateway model id:

```json
{
  "model": "gpt-55-bedrock",
  "input": "Write a concise migration note."
}
```

Use `/v1/chat/completions` only for routes configured with a Chat Completions API style. GPT-5.5 on Bedrock is configured here as Responses-first with `mantle_openai_responses`.
