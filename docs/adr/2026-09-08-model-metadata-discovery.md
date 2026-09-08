# Cached model metadata discovery

Status: Accepted

## Decision

Expose authenticated discovery through `GET /v1/model-metadata`, with an explicit response schema version. Keep `/v1/models` compatible with its existing contract. Reuse model access checks, explicit catalog identities, cached catalog refresh, and adapter capability rules.

Store descriptive capabilities, reasoning options, and conditional prices in the existing catalog snapshot JSON. These fields do not participate in the pricing history or ledger. No database migration is required. The projection cache key advances to `models_dev_supported_v3` so an old ETag cannot keep the previous projection indefinitely. Older vendored snapshots still deserialize with unknown metadata until a successful refresh.

Resolve alias targets and fetch routes and providers in batches. Aggregate limits and capabilities conservatively across enabled routes. Keep prices at route level. Reads never refresh either source or probe a provider.

## Source merge

models.dev remains primary. A separate checked-in LiteLLM snapshot supplements missing fields and records conflicts at read time. The first mapping is deliberately limited to exact OpenAI model IDs with `litellm_provider: openai` and no regional qualifier. Unsupported namespaces remain unknown. No alias, URL, region, or tier inference is permitted.

Maintain the snapshot with `mise exec -- python3 scripts/sync-model-metadata.py MODELS_DEV_JSON LITELLM_JSON crates/gateway-service/data/model_metadata_supplement.json`. The inputs are local downloads of [models.dev](https://models.dev/api.json) and the [LiteLLM catalog](https://raw.githubusercontent.com/BerriAI/litellm/refs/heads/litellm_internal_staging/model_prices_and_context_window.json). Review the generated diff before committing. The generator records hashes and converts per-token rates with decimal arithmetic. Run `mise exec -- python3 scripts/test_sync_model_metadata.py` to check the importer.

The supplement is a deployment artifact, so it can be older than the refreshed primary catalog. Its generation time and hashes remain visible. Source merge reports describe catalog inputs; configured route overrides and caps are applied afterward. Conditional source prices remain informational and never become unconditional ledger prices.

## Trade-offs and follow-up

A separate endpoint requires explicit client integration but avoids changing the OpenAI list contract. An immutable supplement gives maintainers a review gate without introducing another runtime fetch or changing pricing reconciliation. Broader LiteLLM namespaces require exact provider, region, and tier mappings plus tests before inclusion.

A Codex-native catalog export is deferred until there is a verified consumer contract and a need beyond the existing TOML export. Do not derive harness instructions from third-party model metadata.

Validation: service catalog and aggregation tests, an authenticated handler test, importer tests, workspace Clippy, Rust formatting, and `mise run //docs:build`.
