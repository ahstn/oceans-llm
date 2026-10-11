# Model Benchmark Snapshot

`See also`: [Configuration Reference](../../configuration/configuration-reference.md#benchmark-catalogue), [ADR: OpenRouter benchmark snapshot](../../adr/2026-09-24-openrouter-benchmark-snapshot.md)

This page is maintainer-facing. It explains how the vendored benchmark snapshot behind the admin Models page is refreshed.

## Source of Truth

- vendored snapshot: [crates/gateway-service/data/model_benchmarks.json](../../../crates/gateway-service/data/model_benchmarks.json)
- sync binary: [crates/gateway/src/bin/sync_model_benchmarks.rs](../../../crates/gateway/src/bin/sync_model_benchmarks.rs)
- parsing, merging, and matching: [crates/gateway-service/src/benchmark_catalog.rs](../../../crates/gateway-service/src/benchmark_catalog.rs)

The gateway embeds the snapshot at build time, so a refresh only reaches deployments through a new release.

## Refreshing

```bash
mise run sync-model-benchmarks
```

The task reads OpenRouter's top 200 models by intelligence and then:

- skips `:variant` IDs such as `:batch` and `:free`, and models without Artificial Analysis indices
- fails if the response is empty, larger than 8 MiB, contains out-of-range scores, or contains no benchmark data at all
- upserts entries and never removes them, so models that drop out of the top 200 keep their last known scores
- keeps a previously known index when OpenRouter now reports it as `null`
- changes a model's `updated_at` only when its indices change

Review and commit the resulting JSON diff. Keep the `_metadata.attribution` text intact: Artificial Analysis attribution, retrieved via OpenRouter, must stay visible wherever scores are shown.
