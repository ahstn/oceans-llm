# ADR: Image Generation Models

- Date: 2026-10-26
- Status: Accepted
- Issue: [#576](https://github.com/ahstn/oceans-llm/issues/576)

## Current state

- [Model Routing and API Behavior](../configuration/model-routing-and-api-behavior.md), for the Images API routes and the `images` route capability
- [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md), for provider-reported cost and `image_output_usd_per_million_tokens`
- [Observability and Request Logs](../operations/observability-and-request-logs.md), for the image and reasoning large-field paths
- [Verification recipe: Image generation](../../.agents/skills/verify-oceans-admin/features/image-generation.md)

## Context

A live check against OpenRouter on 2026-10-26 found four gaps for image models:

- GPT Image, FLUX, Recraft, Seedream and other image-only models are served only on OpenRouter's `POST /api/v1/images`. The gateway had no Images API route, so these models could not be called.
- Gemini-style image models already worked on `/v1/chat/completions`, but the gateway priced their output as text. One `gemini-3.1-flash-lite-image` generation cost $0.0336 at OpenRouter (1,120 image tokens at $30 per million). The catalog output rate ($1.50 per million) prices the same tokens at $0.0017, so budgets under-counted by a factor of about 20.
- Image responses are 1–2.5 MB: a base64 image plus about 1 MB of encrypted `reasoning_details`. Neither was a known large field, so the request log replaced the whole response with one truncated preview string and lost `usage`.
- `/v1/models` showed `null` output modalities for `gpt-image-2.5-flare`, because models.dev's OpenRouter catalog does not list it.

models.dev, the pricing catalog source, publishes no image-output rate. For aggregator entries such as OpenRouter's, its `output` rate is the text rate. With a BYOK provider key, OpenRouter reports `usage.cost: 0` and puts the provider bill in `usage.cost_details.upstream_inference_cost` ($0.00694 for one low-quality GPT Image request).

## Decision

### 1. Images API route

`POST /v1/images` and `POST /v1/images/generations` share one handler. The unversioned `/images` and `/images/generations` aliases call the same handler, like `/embeddings`. The handler follows the embeddings flow: inference-scope authentication, model resolution and API-key grants, inference rate limits, route selection, prompt guardrails, the pre-provider budget check, usage accounting, and request logs. The request body has typed `model` and `prompt` fields. Every other field (`input_references`, `resolution`, `aspect_ratio`, `quality`, `n`) passes through unchanged. The provider call goes to `{base_url}/images` on `openai_compat` providers and returns the upstream JSON.

The route is non-streaming. A body with `stream: true` gets 400.

### 2. Opt-in `images` route capability

`ProviderCapabilities.images` is a new capability. It is off by default in route configuration, like `decisions`, and config validation accepts it only on `openai_compat` and `gcp_cloud_run_openai_compat` providers. Other providers keep the default `ProviderClient::images`, which returns `not_implemented`.

An image-only route sets `images: true` and turns the other API families off. Route selection then rejects a Chat Completions request to that model with the usual "no configured route supports requested capabilities" error, instead of an upstream 404.

### 3. Provider-reported cost first

When the provider usage has `cost`, the gateway records it as the request cost with `pricing_source: provider_reported`. When `usage.is_byok` is true, it records `cost + cost_details.upstream_inference_cost`: OpenRouter's own fee plus what the provider bills the operator's key. The amounts are parsed as exact decimals, summed, and rounded half up once to four decimal places. A malformed or negative amount, or a BYOK report without the upstream amount, logs a warning and falls back to token rates.

Only when the usage has no reported cost does the gateway use the catalog or the route `pricing_override`. `usage.completion_tokens_details.image_tokens` (or `output_tokens_details.image_tokens`) are then charged at the new optional `pricing_override.image_output_usd_per_million_tokens`, and the other completion tokens at the output rate. The catalog has no image rate. When no image rate exists, all completion tokens use the output rate, as before. The ledger stores `image_output_tokens` and the image rate on `usage_cost_events`.

### 4. Image-safe request logs

New large-field paths cover chat `choices[].message.images[].image_url.url`, streaming `delta.images[].image_url.url`, assistant `messages[].images[].image_url.url` in requests, Images API `data[].b64_json` and `input_references[].image_url.url`, and `reasoning_details[].data` in responses, stream deltas and request history. Each match becomes `{truncated, size_bytes, preview}`, so the stored payload keeps its JSON structure and `usage`.

### 5. Image output modality

`/v1/models` and the admin model metadata add `image` to the output modalities of a model with an `images` route, also when the catalog has no modality data. A model with a separate chat route keeps `text` output. `GET /model_group/info` reports `mode: image_generation` for an image-only model, so LiteLLM clients do not offer it for chat. The admin model view adds `supports_image_generation`, which is true when any eligible route serves images, and the Models page shows an `Image Generation` badge.

## Why

- Provider-reported cost matches the bill exactly, including BYOK, and needs no image-rate source that does not exist yet. It also fixes chat image models on OpenRouter without any per-model configuration.
- Summing the BYOK fee and the upstream inference cost records the real total spend. Recording only the upstream cost would leave out OpenRouter's fee; recording only `cost` would record $0.
- An opt-in capability keeps every `openai_compat` route from claiming image support, gives route selection a reason to reject chat requests to image-only models, and gives `/v1/models` a source for the `image` output modality.
- Large-field paths keep the request log useful for audit and debugging without storing megabytes of base64.

## Trade-offs

- **Provider-reported cost overrides `pricing_override`.** A route with a configured override, such as a negotiated rate or an internal markup, now records the provider's reported cost when the provider sends one. Before this change the override was always authoritative. Operators who need the override must use a provider that does not report cost.
- **The recorded cost depends on the provider.** A provider that reports a wrong `cost` is recorded as-is. Today only OpenRouter-style providers report it.
- **Four decimal places.** A very small reported cost rounds to `$0.0000` or differs by up to $0.00005, the same as catalog pricing.
- **No catalog image rate.** Catalog-priced routes without a reported cost, such as a native provider without an override, still charge image tokens at the output rate. For native OpenAI and Google entries in models.dev the `output` rate is already the image rate, but for aggregator entries it is the text rate.
- **Images API path.** The provider call uses `{base_url}/images`, which is OpenRouter's path. Native OpenAI serves `/images/generations` with a different body, so a native OpenAI route needs follow-up work.
- **Non-streaming only.** OpenRouter's Images API can stream; the gateway rejects `stream: true`.

## Follow-ups

- Add an image-output rate to the pricing catalog when a source publishes one, for example OpenRouter's public `/models` `image_output` field.
- Support Images API streaming.
- Support native OpenAI `/images/generations` and native Vertex or Bedrock image models.
- Translate chat `images` into `/v1/messages` and Responses API outputs.
