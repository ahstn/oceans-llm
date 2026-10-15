# ADR: Model and Provider Config Fragments and Inferred Fields

- Date: 2026-10-15
- Status: Accepted
- Builds On:
  - [2026-10-14-team-scoped-identity-config-fragments.md](2026-10-14-team-scoped-identity-config-fragments.md)
  - [2026-03-10-model-aliases-and-provider-route-config.md](2026-03-10-model-aliases-and-provider-route-config.md)
  - [2026-03-13-capability-aware-route-gating.md](2026-03-13-capability-aware-route-gating.md)
  - [2026-07-16-route-level-effective-model-metadata-overrides.md](2026-07-16-route-level-effective-model-metadata-overrides.md)
  - [2026-10-04-shared-model-routing-and-session-affinity.md](2026-10-04-shared-model-routing-and-session-affinity.md)

## Current state

- [../configuration/configuration-reference.md](../configuration/configuration-reference.md#config-files-and-fragments)
- [../configuration/model-routing-and-api-behavior.md](../configuration/model-routing-and-api-behavior.md#gate-routes-by-capability)
- [../setup/kubernetes-and-helm.md](../setup/kubernetes-and-helm.md#config-fragments-from-your-own-configmaps)
- [../setup/runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md)

## Context

`providers` and `models` lived in one file as lists. That caused four problems:

- **No way to split the file.** The gateway read one file. A missing file silently produced an empty config. Helm replaces lists across `-f` values files, so `gateway.config.models` could not be split across values files either.
- **Silent mistakes.**
  - Duplicate model ids were accepted: validation kept the last entry and seeding upserted both.
  - Routes, route `capabilities` and `compatibility`, and six of seven provider types ignored unknown fields, so a typo such as `capabilites:` did nothing.
  - Guardrails were validated against model route keys before the models themselves.
- **Repeated values.** Admins restated what other config already decided. Across the checked-in configs:
  - 11 of 12 `pricing_provider_id` values followed from the provider host.
  - 11 `auth.kind: bearer` lines set a field nothing read.
  - Every route `capabilities` block in `gateway.yaml` repeated what the Bedrock `api_style`, OpenRouter Decisions API, or Copilot `upstream_supports` already implied. Validation forced most of those values anyway.
  - 19 of 20 provider-backed models had a single route.
  - Two of five route overrides equalled the catalog.
- **Unstable route identity.** A route without `id` got an identity from its priority and YAML position. Reordering routes created new routes, which reset session affinity and round-robin cursors.

The request path and the model-metadata path also kept separate tables of what each provider type can serve.

## Decision

### 1. Keyed collections and single-route shorthand

`providers` and `models` accept either the existing list form or a map keyed by id. A map entry must not also set `id`. A model may use `route:` for a single route; setting both `route` and `routes` is an error.

The model id is never inferred. It is the stable key behind grants, budgets, request logs, and spend (`model_uuid(id)`), so deriving it from `upstream_model` would rename a model whenever its upstream changed.

### 2. Strict fields and duplicate ids

The following are now validated:

- **Unknown fields are rejected** on:
  - routes, `capabilities`, `compatibility`, `compatibility.openai_compat` and `compatibility.aws_bedrock`;
  - every provider type and its auth, display, timeout and batch blocks.
- **Duplicate ids are rejected.** A provider or model id defined twice is an error, whether both definitions are in one file or in two. Guardrails are validated after models.

### 3. Fields inferred from other config

| Field | Inferred from |
| --- | --- |
| `pricing_provider_id` (`openai_compat`, `anthropic_compat`) | `base_url` host: `api.openai.com` is `openai`, `openrouter.ai` is `openrouter`, `opencode.ai` is `opencode`. Required for any other host. |
| `auth.kind` (`openai_compat`, `typesafe`) | Optional, defaults to `bearer`, and is validated when set. |
| Route `id` | The provider key, when that provider appears once in the model and the key is a valid route id. |
| Route `capabilities` | The provider type and compatibility profile (decision 4). |
| Bedrock `openai_base_path` | `/openai/v1` for `runtime_openai_chat`, `/v1` for the Mantle OpenAI styles, per AWS's documented base URLs. Mantle models served under `/openai/v1`, such as GPT-5.5, still set it. |
| Provider icon | `aws_bedrock` type maps to the `aws` icon. |

Inference runs once after files are merged and before validation. It never consults the pricing catalog, which refreshes at runtime, so validation stays deterministic and offline.

### 4. Route capabilities default from the profile

Each `capabilities` flag is optional. An omitted flag takes the value implied by the route:

- **Decisions transports** (`typesafe` and OpenRouter `api: decisions`): `decisions` only.
- **Copilot routes:** `github_copilot_route_capabilities` from the route's compatibility block.
- **Bedrock routes:** the `api_style` rules.
- **Any other route:** everything except `decisions`.

An explicit flag replaces the derived value. Validation still rejects flags a profile cannot serve. At request time route flags are still intersected with provider capability.

The per-provider-type baselines now live once in `gateway-core`: `ProviderCapabilities::for_provider_type` and `route_provider_capabilities`. Provider clients and model metadata both use them.

### 5. Config directory

`--config-dir` (`GATEWAY_CONFIG_DIR`) adds fragments to the main file.

**Which files are read:**
- `*.yaml` and `*.yml` files, read recursively in lexical order of relative path.
- Dot entries are skipped, which covers Kubernetes `..data` directories. Symlinks are followed.

**What a fragment may contain:**
- Only `providers` and `models`.
- Fragments merge as a union, so an id defined in two places is an error that names both files.

**Missing paths:**
- An explicitly set `--config` or `--config-dir` must exist.
- Only the implicit `./gateway.yaml` may be absent.

`gateway config render --show-sources` prints the merged `providers` and `models` in map form and lists each one's source file. It redacts `literal.` values, header values, and URL passwords.

The suggested layout is `models/<provider-account>.yaml` for a provider account and the models that route only to it, plus `models/multi-provider.yaml` for models whose routes span providers. [`deploy/examples/config-dir`](../../deploy/examples/config-dir) is a tested example.

### 6. Helm

The chart supports two ways to split model config.

**Map form:**
- `gateway.config.providers` and `models` default to `{}`, so maps merge across `-f` values files.
- An overlay deletes an entry with `<id>: null`.

**ConfigMaps you create:**
- `gateway.configDir.configMaps` mounts each one at `<mountPath>/<name>/`, using one volume per ConfigMap. The identity ADR proposed a projected volume, but separate mounts keep key names scoped to their ConfigMap.
- The chart sets `GATEWAY_CONFIG_DIR` and mounts the fragments into the gateway pods and all hook Jobs.

### 7. Startup warning for redundant overrides

At startup the gateway logs a warning for each route whose `pricing_override` or `context_window_tokens` equals the current catalog value. The same pass that rejects a context window above the catalog limit does this, resolving each overridden route once. Such an override stops the route from following catalog updates. `EffectiveRouteMetadata` now carries `catalog_pricing` alongside the effective pricing so this comparison is possible.

## Implementation

- [../../crates/gateway/src/config/collections.rs](../../crates/gateway/src/config/collections.rs): map-or-list deserialization.
- [../../crates/gateway/src/config/loader.rs](../../crates/gateway/src/config/loader.rs): `ConfigSources`, fragment discovery, union merge, provenance, `render_config`.
- [../../crates/gateway/src/config/models.rs](../../crates/gateway/src/config/models.rs), [routes.rs](../../crates/gateway/src/config/routes.rs) and [providers.rs](../../crates/gateway/src/config/providers.rs): shorthand, inference, derived capabilities, strict structs.
- [../../crates/gateway-core/src/domain.rs](../../crates/gateway-core/src/domain.rs): shared capability baselines.
- [../../crates/gateway/src/cli.rs](../../crates/gateway/src/cli.rs): `--config-dir` and `config render`.
- [../../deploy/helm/oceans-llm](../../deploy/helm/oceans-llm/README.md): `gateway.configDir` and map defaults.
- Checked-in configs use the map form and drop inferred values.

## Consequences

Positive:

- Model config can be split by provider account in plain files, Helm values files, or ConfigMaps, without the chart knowing file names.
- Typos and duplicate ids fail at load time, naming the files involved.
- A typical model entry is a key, a provider, and an upstream model.
- Route identities no longer change when routes are reordered.
- The request path and model metadata agree on provider capability.

Trade-offs:

- **Route identity changes once.**
  - Unnamed routes now take their provider key as id, so the first reseed after upgrading gives them new internal route ids.
  - Old route rows not referenced by batch jobs are deleted, and new ones are inserted.
  - A batch-referenced row keeps its old id. Seeding now reuses any surviving row with the same model, provider, upstream model, and priority instead of inserting a second row that the `model_routes` unique key would reject. Before this change, renaming a route with batch history failed the seed.
  - Session affinity bindings for those routes reset once.
- **Unknown fields now fail.** Configs with misspelled or stale provider and route fields fail to load until those fields are fixed.
- **A missing explicit config path fails.** Deployments that pointed `GATEWAY_CONFIG` at a missing file and relied on the empty default now fail at startup.
- **List form in Helm is noisy.** List-form `providers` and `models` in Helm values log a Helm notice, because the chart defaults are now maps.
- **External fragment changes are not detected.** The chart cannot checksum ConfigMaps it does not render, so changing one does not restart pods or reseed. A reseed needs the seed-config Job.
- **Removing a fragment removes nothing.** Seeding still never prunes providers or models, and the routes of a deleted model stay enabled.

## Follow-up

- **Ownership and pruning.** Extend the identity ADR's ownership marker and opt-in pruning to providers and models, so that removing a fragment can disable what it declared.
- **Transactional seeding.** Seed providers, models and routes in one transaction.
- **Identity collections in fragments.** Allow `teams`, `users` and `service_accounts` in fragments once the identity ADR's map shape lands.
- **Tag selectors for `allowed_models`.** Consider letting service-account `allowed_models` select model `tags`, so key grants need not repeat every model id.

## Attribution

This ADR was prepared through collaborative human + AI design and implementation work.
