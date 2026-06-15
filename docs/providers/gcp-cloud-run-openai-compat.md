# Google Cloud Run OpenAI-Compatible Models

`See also`: [Configuration Reference](../configuration/configuration-reference.md), [Model Routing and API Behavior](../configuration/model-routing-and-api-behavior.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Google Vertex AI](gcp-vertex.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md)

This page owns provider-specific configuration examples for private Cloud Run services that expose an OpenAI-compatible `/v1` API, such as vLLM-hosted Gemma deployments.

## Current Runtime Boundary

Use `gcp_cloud_run_openai_compat` when the upstream service:

- is deployed on Cloud Run
- exposes OpenAI-compatible endpoints such as `/v1/chat/completions`
- requires Cloud Run IAM authentication with a Google-signed OIDC ID token

Use `openai_compat` for arbitrary OpenAI-compatible endpoints with static bearer-token auth. Use `gcp_vertex` for Vertex AI publisher endpoints. Cloud Run OpenAI-compatible routes reuse the OpenAI-compatible request, response, and stream normalization path; only upstream authentication is Cloud Run-specific.

## Provider

```yaml
providers:
  - id: gemma-cloud-run
    type: gcp_cloud_run_openai_compat
    base_url: https://gemma-service-abc-uc.a.run.app/v1
    pricing_provider_id: google-vertex
    auth:
      mode: adc
    display:
      label: Gemma on Cloud Run
      icon_key: vertexai
```

`base_url` must use `https`. When `audience` is omitted, the gateway derives the Cloud Run audience from the service origin. For example, `https://gemma-service-abc-uc.a.run.app/v1` becomes `https://gemma-service-abc-uc.a.run.app/`.

Set `audience` when the service uses a Cloud Run custom audience:

```yaml
providers:
  - id: gemma-cloud-run-custom-audience
    type: gcp_cloud_run_openai_compat
    base_url: https://gemma.example.com/v1
    audience: https://custom-audience.example.com
    pricing_provider_id: google-vertex
    auth:
      mode: adc
```

## Auth Modes

`adc` uses Application Default Credentials. In Google Cloud runtimes with an attached service account, the gateway uses the metadata server identity endpoint to mint an audience-scoped ID token. When ADC points at a service-account JSON file, the gateway uses the service account's OAuth token URI with a signed JWT assertion that includes `target_audience`.

```yaml
auth:
  mode: adc
```

`service_account` reads a mounted service-account JSON file and uses the service account's OAuth token URI with a signed JWT assertion that includes `target_audience` for the configured or derived audience.

```yaml
auth:
  mode: service_account
  credentials_path: /var/run/secrets/gcp/service-account.json
```

`bearer` is only for constrained debugging environments where an operator has already minted an ID token. The token is treated as static bearer material and is not refreshed.

```yaml
auth:
  mode: bearer
  token: env.CLOUD_RUN_ID_TOKEN
```

Do not put service-account JSON or short-lived ID tokens directly in `gateway.yaml`. Use mounted files or environment references.

## Auth Header

The default upstream auth header is `Authorization: Bearer <token>`.

Use `auth_header: x_serverless_authorization` when a Cloud Run proxy or frontend needs the original `Authorization` header for application-level auth:

```yaml
providers:
  - id: gemma-cloud-run
    type: gcp_cloud_run_openai_compat
    base_url: https://gemma-service-abc-uc.a.run.app/v1
    pricing_provider_id: google-vertex
    auth_header: x_serverless_authorization
    auth:
      mode: adc
```

## Route Example

Cloud Run vLLM routes are OpenAI-compatible routes. Use route `extra_body` for vLLM/Gemma request controls that are additive provider parameters:

```yaml
models:
  - id: gemma-cloud-run
    description: Gemma served by vLLM on private Cloud Run
    tags: [cloud-run, gemma]
    routes:
      - provider: gemma-cloud-run
        upstream_model: google/gemma-4-12b-it
        capabilities:
          chat_completions: true
          responses: false
          embeddings: false
        extra_body:
          chat_template_kwargs:
            enable_thinking: true
          skip_special_tokens: false
```

Keep route capability flags aligned with the deployed vLLM server and tested gateway behavior. The current provider path reuses OpenAI-compatible Chat Completions, streaming, Responses, and embeddings request handling, but a given Cloud Run service might only expose some of those endpoints.

## IAM Notes

- Grant the calling gateway identity `roles/run.invoker` on the receiving Cloud Run service.
- The ID-token audience must match the Cloud Run service URL or a configured custom audience.
- Tokens are cached and refreshed before expiry.
- `auth.mode: adc` is preferred for workloads running on Google Cloud.
- `auth.mode: service_account` is useful when a mounted JSON key is the deployment constraint.

## Pricing And Budgets

Cloud Run OpenAI-compatible providers require `pricing_provider_id`, matching ordinary `openai_compat` providers. The gateway uses this pricing identity plus `upstream_model` for catalog lookup. If the model is not present in the catalog, the request remains visible as `unpriced`; the gateway does not guess self-hosted inference cost.

Budgets are still configured for users, service accounts, and user-model scopes. Provider auth does not create a budget principal. See [Budgets](../access/budgets.md) for user-facing setup.

## Validation

For provider changes, run:

```bash
cargo test -p gateway-providers id_token
cargo test -p gateway-providers x_serverless
cargo test -p gateway cloud_run_openai_compat
mise run lint
mise run docs:check
```
