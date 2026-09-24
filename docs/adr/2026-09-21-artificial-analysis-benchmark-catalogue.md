# ADR: Artificial Analysis benchmark catalogue

- Date: 2026-09-21
- Revised: 2026-09-24
- Status: Accepted

## Context

The Models page shows price, limits, routes, and runtime capability metadata. It does not show an independent capability score.

The first draft of this feature bound gateway models to Artificial Analysis UUIDs from the authenticated Free V2 API. It stored scores in the database and refreshed them on a schedule. Operators had to look up an opaque UUID for every model, and matching gateway models to Artificial Analysis names was unreliable.

OpenRouter's public `GET /api/v1/models` response includes Artificial Analysis Intelligence, Coding, and Agentic indices for each model under `benchmarks.artificial_analysis`. It keys them by readable, routable IDs such as `anthropic/claude-sonnet-4.6`, which are close to the upstream model IDs the gateway already configures.

## Decision

### 1. Scores are a vendored JSON snapshot

`crates/gateway-service/data/model_benchmarks.json` is committed to the repository and embedded at build time, following the vendored pricing catalogue pattern. The gateway makes no network calls for benchmarks, needs no API key, and has no benchmark tables, migrations, scheduler, or refresh endpoint.

`mise run sync-model-benchmarks` fetches `https://openrouter.ai/api/v1/models?sort=intelligence-high-to-low&limit=200`. It does not paginate further. It skips `:variant` IDs such as `:batch` and `:free`, which config bindings cannot reference, and models with no Artificial Analysis index, and validates that every value is finite and between 0 and 100.

### 2. The snapshot is append and update only

The sync upserts fetched models and never deletes entries. A model that falls out of the top 200 keeps its last known scores. A `null` index in a new fetch keeps the previously stored value. An entry's `updated_at` changes only when its data changes, so unchanged syncs produce no diff. The trade-off is that the snapshot can hold stale scores for models that are no longer ranked.

The `_metadata` block repeats the Artificial Analysis attribution, the benchmark source, and the OpenRouter source URL.

### 3. Model identity is an explicit binding or an exact derived match

A gateway model can set `benchmark_model_id` to an OpenRouter model ID. That binding wins, and an alias inherits its target's binding.

Without a binding, the gateway normalizes the primary route's `upstream_model` into candidate OpenRouter IDs. Normalization strips Bedrock ARNs, region prefixes, and version suffixes, Vertex `@version` suffixes, and OpenRouter `:variant` suffixes. It also maps Bedrock publishers, infers publishers for bare IDs, and tries the dotted version form (`claude-sonnet-4-6` becomes `claude-sonnet-4.6`). A candidate must match a snapshot key exactly. There is no prefix or fuzzy matching, because related variants such as `deepseek-v4-pro` and `deepseek-v4-pro-0813` have different scores.

Each score reports whether it was `explicit` or `derived`, so operators can see where a binding came from.

### 4. The admin API owns display metadata

`GET /api/v1/admin/models` returns benchmark scores to the admin UI. The OpenAI-compatible `/v1/models` response does not change. Attribution to Artificial Analysis, retrieved via OpenRouter, is shown below the Models list and with every detailed score.

## Consequences

Benefits:

- most configured models get scores without any config change
- three indices instead of one
- model list reads cannot fail or slow down because of a benchmark source
- no secrets, storage, or background jobs

Trade-offs:

- scores only change when someone runs the sync and commits the result
- models outside OpenRouter's top 200 by intelligence are not added
- a derived match can still attach a score to a differently configured deployment of the same model; use `benchmark_model_id` to override it

## Attribution

This ADR was prepared through collaborative human + AI implementation and design work.
