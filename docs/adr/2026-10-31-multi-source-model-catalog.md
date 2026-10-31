# ADR: Multi-Source Model Catalog and Tri-State Route Capabilities

- Date: 2026-10-31
- Status: Accepted
- Supersedes: [Cached model metadata discovery](2026-09-08-model-metadata-discovery.md)
- Amends: [Hybrid Pricing Catalog from models.dev](2026-03-06-hybrid-pricing-catalog.md)

## Current state

- [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md), for catalog sources, refresh, context tiers, and the catalog image rate
- [Model Routing and APIs: Gate routes by capability](../configuration/model-routing-and-api-behavior.md#gate-routes-by-capability), for tri-state capabilities, inference, and catalog hints
- [Configuration Reference: Route Config](../configuration/configuration-reference.md#route-config), for capability syntax and validation
- [Request Lifecycle and Failure Modes: Model metadata discovery](../reference/request-lifecycle-and-failure-modes.md#model-metadata-discovery), for `/v1/model-metadata` schema version 2

## Context

The gateway had one runtime catalog source, models.dev. This caused these problems:

- **Tiered prices were lost.** models.dev lists context tiers, but the projection dropped them. A `gpt-6-astra` request above 272,000 prompt tokens was charged $10 input and $50 output per million tokens instead of $20 and $75.
- **Image-only models had no data.** models.dev does not list OpenRouter image models such as `openai/gpt-image-2.5-flare`, and it publishes no image output rate.
- **A static supplement filled gaps.** A checked-in LiteLLM supplement covered only exact OpenAI model IDs. A Python script built it, and it was not refreshed at runtime.
- **Route capabilities were hand-written.** Every capability flag was a plain `bool` with a permissive default. Admins had to write a full `capabilities` block on each partial route. Bedrock routes had to repeat facts that the `api_style` already fixes.
- **Catalog fetches had loose limits.** The fetch had no connect timeout and no body size cap. The refresh ran every 15 minutes.
- **GitHub Copilot had no pricing identity.** models.dev has a `github-copilot` provider, but the gateway did not project it.

## Decision

### 1. Three catalog sources

The gateway reads three sources. Each source has its own cache row in `pricing_catalog_cache`, its own vendored fallback, and its own change check.

| Source | URL | Cache key | Size cap | Change check | Vendored fallback |
| --- | --- | --- | --- | --- | --- |
| models.dev | `https://models.dev/api.json` | `models_dev_supported_v4` | 32 MiB | ETag | `pricing_catalog_fallback.json` |
| OpenRouter | `https://openrouter.ai/api/v1/models?output_modalities=all` | `openrouter_v1` | 16 MiB | Content checksum | `openrouter_catalog_fallback.json` |
| LiteLLM | `https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json` | `litellm_v1` | 16 MiB | ETag | `litellm_catalog_fallback.json` |

The vendored files are in `crates/gateway-service/data/`. `mise run //crates/gateway:sync-pricing-catalog` refreshes all three, and the release task runs it.

Each source projects into the same document: gateway catalog provider ID, then model ID, then one model document. The provider IDs are `openai`, `openrouter`, `amazon-bedrock`, `google-vertex`, `google-vertex-anthropic`, `opencode`, and `github-copilot`. Sources match exact model IDs only. There is no fuzzy matching.

- models.dev keeps its existing projection and now also projects `github-copilot`.
- OpenRouter projects only the `openrouter` provider, as `openrouter/<id>`. The `output_modalities=all` query includes image-only models. A `"-1"` price means a variable price, so that pricing schedule is skipped.
- LiteLLM uses exact key rules: bare `openai` keys without regions, `github_copilot/<model>`, Bedrock bare keys and `bedrock_mantle/<model>`, and `vertex_ai/<model>` split into Google and Anthropic publishers. It has no `openrouter` entries.

### 2. Per-field precedence

The merge is a pure function. For each provider and model, each field takes the value of the first source that has one:

1. OpenRouter, only for the `openrouter` provider
2. models.dev
3. LiteLLM

An explicit `false` or zero is a value, so it wins. When two sources disagree, the merge records a conflict. It does not pick the smaller value.

Pricing is one whole schedule: input, output, cache read, cache write, audio, image output, and context tiers. The schedule comes from the first source that has both an input and an output rate. Rates from two sources never mix. When no source has both, the first source with an input or an output rate supplies the schedule. This keeps input-only embedding prices.

Billing uses the same merged view as metadata and discovery.

### 3. Refresh and reconciliation

- **Interval.** Each source refreshes when its snapshot is older than 6 hours. Startup runs the same check before the gateway serves requests.
- **Bounded fetch.** Each request has a 5-second connect timeout and a 30-second total timeout. A body above the source's size cap fails the refresh.
- **No-change detection.** models.dev and LiteLLM use `If-None-Match`. OpenRouter sends no ETag, so the gateway compares a checksum of the projected document. An unchanged source writes nothing.
- **Failure isolation.** A source that fails keeps its cached snapshot, or its vendored snapshot when no cache row exists. The other sources still refresh.
- **One reconciliation.** After any source changes, the gateway rebuilds the merged snapshot and reconciles it into `model_pricing` once, as one generation. Each row records the snapshot of the source that won its pricing: source name, ETag or checksum, and fetch time. Rows that did not change are not rewritten.
- **Manual refresh.** **Refresh pricing** on the Models page forces all three sources. `POST /api/v1/admin/models/pricing-catalog/refresh` returns one entry per source (`models_dev`, `openrouter`, `litellm`) with `outcome` (`updated`, `unchanged`, or `failed`), the `fetched_at` of the snapshot in use, and a stable `error` code. The response has no upstream error text.

### 4. Context tier billing

Catalog prices can have typed context tiers. Each tier has a threshold, `above_input_tokens`, and optional input, output, cache read, and cache write rates.

- models.dev tiers come from `tiers` entries with `type: context`. The `context_over_200k` condition is never used to make a tier. It repeats the first tier, often at a different threshold. Two different tiers at one threshold leave the model with base rates only.
- LiteLLM tiers come from `*_above_<N>k_tokens` keys that have no other suffix. Keys such as `_priority`, `_flex`, `_batches`, and `_above_1hr` are other billing dimensions and are ignored.

The billing rule:

- The tier applies when the normalized prompt tokens, cache tokens included, are **strictly greater** than its threshold. A request of exactly the threshold uses the lower schedule.
- When more than one tier applies, the tier with the highest threshold wins.
- The tier rates apply to the **whole request**, not only to the tokens above the threshold.
- A tier field that is not set uses the base rate for that bucket.
- Order of precedence: provider-reported cost, then the route `pricing_override`, then the catalog. An override is flat and replaces the catalog schedule, tiers included.
- A batch stores the full schedule and selects the tier for each item from that item's prompt tokens.

`usage_cost_events.pricing_tier_above_input_tokens` records the tier that billed the request. It is null for base rates, overrides, and provider-reported cost. Migration V76 adds this column and `model_pricing.context_tiers_json` and `model_pricing.output_image_cost_per_million_tokens_10000`.

### 5. Catalog image output rate

OpenRouter `pricing.image_output` and LiteLLM `output_cost_per_image_token` become the catalog image output rate. When the resolved catalog row has this rate, image output tokens are priced at it and the other completion tokens at the output rate. This is the same split as the override image rate.

### 6. Tri-state route capabilities

Each route capability flag is now `true`, `false`, or not set. The gateway finds the effective value in this order:

1. Explicit route config
2. Compatibility inference
3. Catalog hints
4. Defaults: `images` and `decisions` are off, all other flags are on

The result is then capped by the provider adapter. One function in `gateway-core`, `effective_route_capabilities`, does this. Route selection, batch admission, and the shared effective route metadata behind the admin model views and model listings call it.

Compatibility inference uses only facts that the adapter guarantees:

| Route | Inferred flags |
| --- | --- |
| Bedrock `runtime_converse`, `runtime_openai_chat`, `mantle_openai_chat`, `mantle_anthropic_messages` | `chat_completions` on; `stream` on; `responses`, `json_schema`, `embeddings`, `images`, `decisions` off |
| Bedrock `runtime_anthropic_invoke` | `chat_completions` on; `responses`, `stream`, `json_schema`, `embeddings`, `images`, `decisions` off |
| Bedrock `mantle_openai_responses` | `responses` and `stream` on; `chat_completions`, `embeddings`, `images`, `decisions` off. `tools`, `vision`, and `json_schema` depend on the model, so they stay unset |
| GitHub Copilot with `compatibility.github_copilot` | Every flag, from the Copilot route evidence |
| TypeSafe, `openai_compat.decisions_api`, or `openrouter.api: decisions` | `decisions` on; every other flag off |
| Any other route | Nothing |

Catalog hints come from the merged catalog entry of the route's catalog identity:

| Flag | Hint source |
| --- | --- |
| `chat_completions`, `responses`, `embeddings`, `images` | The entry's API endpoints, when a source states them. LiteLLM uses `supported_endpoints`, or `mode` for `embedding`, `image_generation`, and `responses`. OpenRouter states endpoints only for a model whose output modalities are exactly `["image"]`: images on, the other three off. models.dev does not state endpoints |
| `tools` | `tool_call` |
| `json_schema` | `structured_output` |
| `vision` | `image` in the input modalities, only when the input modalities are known |
| `stream`, `developer_role`, `decisions` | Never hinted |

Hints only fill flags that config and inference leave unset. A hint can set an unset flag to `false`. A hint never changes an explicit value.

Config validation uses explicit values, then inference, then defaults. It does not read hints. It still rejects an explicit value that contradicts inference, such as `stream: true` on `runtime_anthropic_invoke`. An omitted flag no longer fails validation.

`model_routes.capabilities_json` stores only the explicit flags. Rows with all ten flags decode as fully explicit. Config seeding rewrites them at startup, so no SQL migration is needed.

### 7. `/v1/model-metadata` schema version 2

- The single `catalog` and `supplement` provenance fields are replaced by `sources`: one entry per source with `source`, `origin`, `etag`, `checksum`, and `fetched_at`.
- Each route's `merge_report` comes from the merge. `provenance` names the source of each field. `conflicts` lists `{field, winner: {source, value}, others: [{source, value}]}`. It is `null` for a route without a catalog entry.
- `deprecated_date` is removed from route metadata.
- The static supplement is deleted: `model_metadata_supplement.json`, `scripts/sync-model-metadata.py`, its test script, and its merge code.

### 8. Admin display

The admin model API adds `pricing_tiers` for the primary route. The model info dialog shows the base rates and one `Above N tokens` block for each tier. The Models list shows `from $X` for a tiered model. A route with an override has no tiers.

## Implementation

- Capabilities: `crates/gateway-core/src/route_capabilities.rs` (`PartialCapabilities`, `infer_route_capabilities`, `effective_route_capabilities`), `crates/gateway/src/config/routes.rs` (tri-state config and validation), `crates/gateway-service/src/capability_hints.rs` (hint mapping).
- Fetch: `crates/gateway-service/src/catalog_fetch.rs` (timeouts and size cap).
- Sources and merge: `crates/gateway-service/src/pricing_catalog/sources/{mod,models_dev,openrouter,litellm}.rs` and `crates/gateway-service/src/pricing_catalog/merge.rs`.
- Runtime: `crates/gateway-service/src/pricing_catalog.rs` (shared snapshot and 6-hour interval), `pricing_catalog/refresh.rs` (per-source refresh), `pricing_catalog/reconcile.rs` (one-generation reconciliation), `pricing_catalog/target.rs` (pricing provider IDs).
- Billing: `select_context_tier` and `apply_exact_pricing` in `crates/gateway-service/src/service.rs`, batch tiers in `crates/gateway-core/src/batch.rs` and `crates/gateway-service/src/batches.rs`, migration `crates/gateway-store/migrations/V76__pricing_context_tiers.sql` and its PostgreSQL pair.
- Discovery: `crates/gateway-service/src/model_metadata.rs`.
- Vendored snapshots: `crates/gateway/src/bin/sync_pricing_catalog.rs`.
- Admin: `crates/gateway/src/http/models.rs` (refresh response), `crates/admin-ui/web/src/routes/-model-info-dialog.tsx`, and `crates/admin-ui/web/src/routes/models.tsx`.

## Why

- Billing tiered models at base rates under-charged long-context requests. For `gpt-6-astra`, the gateway charged half the correct input rate. Typed tiers fix this for every source that publishes them.
- OpenRouter is the authority for its own catalog. It lists image-only models, image output rates, and model names that models.dev lacks.
- LiteLLM fills limits, flags, and API endpoints for native providers. A runtime source is better than a static supplement that needed a manual script and review.
- A precedence chain is predictable. An admin can see which source won each field. A minimum across sources would mix values that no single source states.
- One whole pricing schedule keeps base rates, cache rates, and tiers consistent. Mixed rates could bill a combination that no provider publishes.
- One reconciliation per generation keeps `model_pricing` history clean. A per-source reconciliation would close and reopen rows each time any source changed.
- Tri-state capabilities remove most `capabilities` blocks. Facts that the adapter guarantees no longer need to be written by hand, and explicit config still wins.
- Fetch limits keep a slow or very large upstream response from blocking startup or using too much memory.
- A 6-hour interval matches how often catalog prices change and reduces load on the sources.

## Trade-offs

- **Precedence, not the minimum.** When models.dev has a larger limit than LiteLLM, the gateway uses the models.dev limit. The conflict is visible in `/v1/model-metadata`, but the gateway does not resolve it.
- **OpenRouter has no ETag.** Each OpenRouter check downloads the full model list, about 1 MB. A checksum of the projected document detects no change, so unchanged data writes nothing.
- **LiteLLM float prices.** LiteLLM publishes per-token prices as JSON floats. The gateway reads the shortest decimal form of each float, multiplies by one million, and rounds to 4 decimal places. A float that does not round-trip cleanly can differ from the published per-million price by up to $0.00005.
- **Partial schedules.** When no source has both input and output rates, the first source with either rate wins. An input-only schedule from a lower source can then bill a model that a higher source lists without prices.
- **Hints follow upstream catalogs.** A hint can turn an unset flag on or off when a source changes. Routing can then change after a refresh without a config change. Admins who need a fixed value set the flag explicitly.
- **Snapshot per request.** Routing loads the merged snapshot once per request. The load locks an async mutex to clone an `Arc`. The lock is held for a short time, except when the snapshot reloads after a source changes.
- **Upgrade path.** The gateway no longer reads the `models_dev_supported_v3` cache row. After an upgrade, each source starts from its vendored snapshot until the first refresh succeeds.
- **Config validation ignores hints.** Validation cannot depend on a catalog that changes at runtime. A route that relies on a hint is checked against the defaults at startup.

## Follow-ups

- Add catalog tiers to `/model_group/info` and to generated client configurations when a client format can carry them.
- Price other tier dimensions, such as priority and flex service tiers, when the request records the tier that the provider used.
- Show source conflicts in the admin model info dialog.
- Add more LiteLLM provider mappings when a gateway provider needs them, each with exact identity tests.
