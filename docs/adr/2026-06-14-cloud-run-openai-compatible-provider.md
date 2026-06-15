# Cloud Run OpenAI-Compatible Provider Auth Boundary

## Status

Accepted.

## Context

Private Cloud Run services can host OpenAI-compatible model servers such as vLLM. Their HTTP shape can match `/v1/chat/completions`, but Cloud Run IAM requires a Google-signed OIDC ID token with an audience matching the receiving service URL or configured custom audience.

That auth shape is different from:

- `openai_compat`, which uses static bearer tokens and default headers for arbitrary OpenAI-compatible endpoints
- `gcp_vertex`, which uses OAuth access tokens with the `cloud-platform` scope and Vertex-specific transport paths

Hiding Cloud Run IAM behind `openai_compat.default_headers` would encourage operators to inject short-lived ID tokens manually and would leave token refresh behavior undefined.

## Decision

Add a first-class provider type named `gcp_cloud_run_openai_compat`.

The provider type:

- reuses the existing OpenAI-compatible request, response, and stream normalization adapter
- makes Cloud Run auth visible in typed config
- supports `auth.mode: adc`, `auth.mode: service_account`, and `auth.mode: bearer`
- derives the ID-token audience from `base_url` when `audience` is omitted
- supports custom audiences through an explicit `audience` field
- sends bearer material through `Authorization` by default, or `X-Serverless-Authorization` when `auth_header: x_serverless_authorization` is configured
- caches ID tokens and refreshes them before expiry through the existing cached token-source abstraction

The ID-token implementation uses:

- the metadata server identity endpoint for ADC on Google Cloud runtimes without a local ADC file
- the service account's OAuth token URI with a signed JWT assertion containing `target_audience` for service-account JSON credentials
- static bearer material only for constrained debugging environments

## Implementation

- `crates/gateway-providers/src/token.rs` owns Cloud Run ID-token sources and JWT expiry parsing.
- `crates/gateway-providers/src/openai_compat.rs` accepts optional cached identity-token auth and configurable bearer header placement.
- `crates/gateway/src/config.rs` parses and validates `gcp_cloud_run_openai_compat`.
- `crates/gateway-service/src/pricing_catalog.rs` prices Cloud Run OpenAI-compatible routes like ordinary `openai_compat` routes by using `pricing_provider_id` plus `upstream_model`.
- `docs/providers/gcp-cloud-run-openai-compat.md` owns operator examples and Cloud Run-specific caveats.

## Trade-Offs

Reusing the OpenAI-compatible provider keeps the transport and stream normalization code in one place, but the runtime provider config now carries a provider type string so the same adapter can register as either `openai_compat` or `gcp_cloud_run_openai_compat`.

Service-account JSON auth signs a JWT assertion locally, then exchanges it at the service account's OAuth token URI for a Google-issued ID token. This follows the default behavior of Google's ID-token credentials while preserving the Cloud Run requirement for Google-issued audience-bound ID tokens.

`auth.mode: bearer` remains available, but only as an explicit debug mode. It is not a fallback for ADC or service-account failures.

## Follow-Ups

- Add service-account impersonation for authorized-user ADC if local operator workflows need it.
- Add deployment examples for Workload Identity on Kubernetes.
- Add exact self-hosted inference pricing support only when a durable cost model exists; do not guess Cloud Run model costs from provider type alone.
