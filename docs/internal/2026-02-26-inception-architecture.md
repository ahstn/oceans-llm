### 1) Competitive Landscape Summary

#### Cross-doc patterns (providers, models, routing, auth)

* **Providers**

  * Common knobs across gateways:

    * **Credentials** via env-var indirection (e.g., `env.FOO`, `os.environ/FOO`) to avoid hardcoding secrets.
    * **Base URL overrides** to support OpenAI-compatible/self-hosted backends.
    * **Per-provider network policy**: retries, backoff, timeouts; sometimes proxy settings; sometimes default headers.

* **Models**

  * Two dominant approaches:

    1. **Gateway model aliases** → map to provider+model (+ metadata like rpm/tpm, region). (LiteLLM; TensorZero)
    2. **Provider-qualified model strings** in the client request (e.g., `openai/gpt-4o-mini`) and the gateway figures out credentials + formatting. (Helicone; Bifrost)

* **Routing**

  * MVP-level routing in competitors usually starts as:

    * **Static mapping** (alias → provider model)
    * **Ordered fallback chains** (try A then B)
  * Advanced routing adds:

    * Weighted balancing across multiple keys/providers
    * Latency/least-busy strategies
    * Inline “routing hints” encoded in the `model` string (Helicone’s approach is particularly compact)

* **Auth**

  * Most products separate:

    * **Gateway auth** (who can call the gateway)
    * **Provider auth** (keys held by the gateway)
  * Mature offerings add:

    * Per-user/team API keys, RBAC, OIDC/JWT
    * Budgets/rate-limits per key/model/team

With that baseline, here’s the competitor-by-competitor breakdown.

---

#### LiteLLM Proxy

* **Core value props**

  * “OpenAI-compatible proxy” + broad provider support.
  * Strong **admin/control-plane** capabilities (keys/users/teams/budgets) when paired with DB and UI.
* **Architecture style**

  * **Combined control plane + data plane** in one service: proxy serves traffic and also manages keys/models (DB-backed).
* **Config approach**

  * YAML `config.yaml` with `model_list` entries mapping `model_name` (alias) to `litellm_params` (provider, base URL, key, etc.).
  * Supports env-var references inside config.
* **Routing capabilities**

  * Load balancing by defining multiple entries for the same `model_name`.
  * Router-level strategies + fallback mappings (including context-window fallbacks).
* **Auth model**

  * `master_key` gating proxy access; “virtual keys” for end-users; extensible custom auth hook.
* **Observability**

  * Logging integrations via callbacks; supports structured diagnostics and “detailed debug”.
* **Cost tooling**

  * Strong spend tracking + budgets/rate limits around keys/users/teams.
* **What to copy**

  * The **alias model registry** pattern (`model_name` → provider params).
  * DB-backed “virtual keys” concept (even if you keep MVP smaller).
  * Explicit `request_timeout`, `num_retries`, and fallback config ergonomics.
* **What to avoid**

  * Overloading the model registry by duplicating the same alias multiple times to express balancing (it becomes hard to reason about); prefer explicit `routes` arrays per model in your design.

---

#### Helicone AI Gateway

* **Core value props**

  * OpenAI-compatible unified API + “provider routing” across many providers/models.
  * Tight integration with Helicone’s observability platform.
* **Architecture style**

  * Clear **data plane** (gateway) with a **control plane** (dashboard/config) in hosted mode.
  * Self-host mode supports config-driven routing.
* **Config approach**

  * Self-hosted YAML supports **routers** (named routing configs) with per-router strategies and rate limits.
  * Uses an internal “model registry” notion and provider lists.
* **Routing capabilities**

  * Provider routing uses a model registry to find providers supporting a requested model, chooses cheapest, balances equal-cost, and fails over on errors.
  * Very compact **model-string routing expressions**:

    * lock to provider via `model/provider`
    * manual fallback chains via comma lists
    * exclusions via `!provider,model`
* **Auth model**

  * Gateway key concept (Helicone API key) and optional auth enablement for self-hosting via a control-plane key.
* **Observability**

  * First-class: logs, costs, latency, errors, etc. (core Helicone product value).
* **Cost tooling**

  * Strong (cost tracking and billing concepts in hosted mode).
* **What to copy**

  * The **router** concept: multiple named routing policies in one gateway instance.
  * The **inline fallback chain syntax** is a good inspiration (even if you implement it via headers instead of `model` parsing).
* **What to avoid**

  * Encoding too much policy into the `model` string for your MVP unless you can validate and test it thoroughly; it becomes a “mini-language” quickly.

---

#### Bifrost Gateway

* **Core value props**

  * OpenAI-compatible gateway with strong **provider configuration primitives**:

    * multiple keys per provider
    * weights (split traffic)
    * per-model key restrictions
    * base URL overrides
    * per-provider retries/backoff, concurrency/buffer
  * Integrated UI and API for configuration.
* **Architecture style**

  * **Single service** that includes both data plane and an embedded control plane (UI + config APIs).
* **Config approach**

  * `config.json` supports a `providers` map with provider objects:

    * `keys[]` each having `name`, `value` (supports `env.` indirection), `models[]`, `weight`
    * optional per-provider `network_config` (base_url, max_retries, backoff, etc.)
* **Routing capabilities**

  * Strong **key-level** routing (weights, model-specific keys).
  * Cross-provider fallback is less central in the docs than provider-qualified requests.
* **Auth model**

  * Has governance “virtual key” and request option headers; gateway-auth is not emphasized in basic curl examples.
* **Observability**

  * Built-in logging/observability with pluggable storage (SQLite default; Postgres supported) and real-time updates.
* **Cost tooling**

  * Aggregated stats in observability/logging (tokens, cost) appear in the logging interface.
* **What to copy**

  * Provider config structure: **keys[] with weights + model allowlists**.
  * Env indirection format `env.VAR`.
  * Clear per-provider network tuning primitives (timeouts/retries/base_url).
* **What to avoid**

  * Shipping a heavy embedded UI in MVP if your priority is a small local-first binary; CLI + config-as-code is typically enough early on.

---

#### TensorZero Gateway

* **Core value props**

  * Rust “gateway” optimized for performance + strong GitOps config.
  * Beyond proxying: structured inference data, optimization workflows, experimentation, caching, etc.
* **Architecture style**

  * **Data plane gateway** plus optional supporting services:

    * Postgres (auth/ops)
    * ClickHouse (observability/analytics)
    * optional Redis/Valkey for caching (depending on deployment)
* **Config approach**

  * TOML configuration as the “backbone”.
  * Models have:

    * `[models.<name>] routing = ["providerA", "providerB"]`
    * `[models.<name>.providers.<provider>] type = "..."; model_name=...; endpoint=...`
  * Strong docs for timeouts, retries/fallbacks, auth caching, OTel export, Prometheus metrics, etc.
* **Routing capabilities**

  * Ordered fallback chains per model.
  * More advanced routing appears via “variants” and experimentation features.
* **Auth model**

  * API key auth (Postgres-backed), with caching of auth DB lookups.
* **Observability**

  * First-class: structured traces stored in ClickHouse; can export OTel traces; Prometheus metrics.
* **Cost tooling**

  * Strong foundation (structured inference records with token counts; UI/analytics patterns).
* **What to copy**

  * The **model block** structure with explicit `routing = [...]` and provider sub-blocks.
  * Clear separation of:

    * “model identity” (gateway alias)
    * “provider access path” (provider config)
    * “routing policy” (ordered list)
  * Ops patterns: `/status` + `/health`, log format toggles, config globs.
* **What to avoid**

  * Pulling “workflow” concepts (prompt templates, episodes, experimentation) into your MVP gateway; keep your first release focused on multiplexing + authZ.

---

#### Other notable competitors (brief)

> Assumption (not deeply researched in this pass due to focus on your requested primary references):

* **OpenRouter**: hosted OpenAI-compatible aggregation across many providers/models; competes on breadth and convenience.
* **Portkey**: gateway + observability + policy features; often used as a managed control plane.

---

### 2) Capability Matrix (MVP vs Competitors)

| Capability                                      | Our MVP (Rust single binary)                                         | LiteLLM                                                   | Helicone                                                                                    | Bifrost                                                          | TensorZero                                               | Other (OpenRouter/Portkey)               |
| ----------------------------------------------- | -------------------------------------------------------------------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- | -------------------------------------------------------- | ---------------------------------------- |
| OpenAI-compatible API                           | **Yes** — `/v1/chat/completions`, `/v1/embeddings`, `/v1/models`     | **Yes** — proxy is OpenAI-compatible                      | **Yes** — OpenAI SDK compatible                                                             | **Yes** — OpenAI request/response format                         | **Yes** — OpenAI compatibility + Responses API support   | **Yes** (assumption) — OpenAI-compatible |
| Multi-provider support                          | **Partial** — start with OpenAI-compat + Vertex; extensible adapters | **Yes** — broad provider list                             | **Yes** — “100+” in docs                                                                    | **Yes** — many providers + custom base URL                       | **Yes** — many providers + OpenAI-compat APIs            | **Yes** (assumption) — broad             |
| Provider creds via env refs                     | **Yes** — `env.VAR` style + file refs                                | **Yes** — `os.environ/VAR`                                | **Yes** — provider keys via `.env` in self-host                                             | **Yes** — `env.VAR`                                              | **Yes** — env vars default for provider creds            | **Yes** (assumption)                     |
| Provider base URL override                      | **Yes** — for OpenAI-compat + custom endpoints                       | **Yes** — `api_base` per model                            | **Partial** — depends on provider settings/config                                           | **Yes** — `network_config.base_url`                              | **Yes** — provider endpoints configurable                | **Yes** (assumption)                     |
| Gateway model registry (alias → provider model) | **Yes** — persisted models + routes                                  | **Yes** — `model_list` aliases                            | **Partial** — often model string is provider-qualified; also has model registry for routing | **Partial** — model strings usually provider-qualified           | **Yes** — `[models.*]` blocks                            | **Partial** (assumption)                 |
| Basic tagging/grouping                          | **Yes** — tags on gateway models + tag-based selection               | **Partial** — model groups by alias, not first-class tags | **Partial** — routing policy oriented, tags not central                                     | **Partial** — governance tags exist; model tags unclear          | **Partial** — variants/functions more than tags          | **Partial** (assumption)                 |
| Static mapping routing                          | **Yes** — alias routes list                                          | **Yes**                                                   | **Yes**                                                                                     | **Yes**                                                          | **Yes**                                                  | **Yes** (assumption)                     |
| Fallback chains                                 | **Yes** — ordered routes per alias                                   | **Yes** — fallbacks config                                | **Yes** — provider routing + manual chains                                                  | **Partial** — more key-routing than provider failover            | **Yes** — ordered `routing = [...]`                      | **Yes** (assumption)                     |
| Load balancing                                  | **Partial** — weight within same priority group                      | **Yes** — routing strategies + weighted picks             | **Yes** — strategies like model-latency                                                     | **Yes** — weights across keys                                    | **Yes** — variants/experiments; also routing options     | **Yes** (assumption)                     |
| Streaming                                       | **Yes** — SSE proxy + normalization                                  | **Yes**                                                   | **Yes**                                                                                     | **Yes**                                                          | **Yes**                                                  | **Yes** (assumption)                     |
| Retries/timeouts                                | **Yes** — safe defaults + configurable                               | **Yes** — `num_retries`, `request_timeout`                | **Yes** — failover triggers + policies                                                      | **Yes** — `network_config.max_retries` etc                       | **Yes** — documented timeouts + retries/fallbacks        | **Yes** (assumption)                     |
| Gateway auth (API keys)                         | **Yes** — Bearer keys, per-model grants                              | **Yes** — master key + virtual keys + OIDC                | **Yes/Partial** — Helicone key; self-host auth via control-plane key                        | **Partial** — governance headers; auth not primary in quickstart | **Yes** — API key auth with caching                      | **Yes** (assumption)                     |
| Rate limiting                                   | **No (later)** — structure won’t block                               | **Yes** — per key/team                                    | **Yes** — per API key in router config                                                      | **Partial** — some governance features exist                     | **Yes** — config supports rules                          | **Yes** (assumption)                     |
| Budgets/spend caps                              | **No (later)**                                                       | **Yes**                                                   | **Yes/Partial** — strong in hosted; unclear self-host parity                                | **Partial** — cost visible in logs; budgets unclear              | **Partial** — strong data model; policy depends on setup | **Yes** (assumption)                     |
| Observability (logs)                            | **Yes (baseline)** — structured tracing logs, redaction              | **Yes** — callbacks + logs                                | **Yes** — built-in observability                                                            | **Yes** — built-in logs store + UI                               | **Yes** — ClickHouse traces + logs                       | **Yes** (assumption)                     |
| OTel traces / Prometheus                        | **No (later)** — add `tracing-opentelemetry` later                   | **Partial** — integrations exist                          | **Partial** — depends on deployment                                                         | **Yes/Partial** — OTel plugin exists                             | **Yes** — OTel + Prometheus export                       | **Yes** (assumption)                     |
| Logs UI                                         | **No (later)**                                                       | **Yes** — proxy UI                                        | **Yes** — dashboard                                                                         | **Yes** — embedded UI                                            | **Yes** — UI available                                   | **Yes** (assumption)                     |
| Local-first single binary                       | **Yes**                                                              | **No** — Python-based                                     | **Partial** — self-host via `npx` wrapper; underlying may be Rust                           | **Partial** — distributed via `npx`; binary form unclear         | **Yes** — Rust gateway binary                            | **No/Partial** (assumption)              |

---

### 3) MVP Product Spec (gateway behavior)

#### Personas

* **Gateway Admin (local-first)**

  * Wants to:

    * configure providers (keys, base URLs, regions/projects)
    * register “gateway models” as stable aliases
    * mint gateway API keys for users/services with least privilege
    * run locally as a single binary

* **Application Developer**

  * Wants to:

    * point existing OpenAI SDK-compatible clients at the gateway
    * use stable `model` names that don’t change when providers change
    * get streaming support and predictable errors

---

#### MVP non-goals

* No UI/dashboard (CLI + config-as-code only)
* No team/SSO/RBAC beyond simple API keys + model grants
* No budgets, rate limits, caching, or cost accounting (but don’t block later)
* No BYOK forwarding (user-supplied provider keys) in MVP
* No prompt/response persistence by default (metadata-only logs)

---

#### API surface

**Recommendation: OpenAI-compatible**, because all reference gateways converge on this (lowest friction for clients).

MVP endpoints:

* `POST /v1/chat/completions`

  * Supports:

    * non-streaming
    * streaming (`stream=true`) with SSE
* `POST /v1/embeddings`
* `GET /v1/models`

  * Lists **gateway models** (aliases) available to the caller (auth-filtered)
* Health:

  * `GET /healthz` (process up)
  * `GET /readyz` (DB + config loaded; optional provider ping in “strict mode”)

Optional-but-useful (still MVP-friendly):

* `GET /version` (build info)
* `GET /metrics` (later; placeholder only in MVP)

---

#### Authentication + request flow

* **Auth method**: `Authorization: Bearer <gateway_api_key>`
* **Key format** (suggested):

  * `gwk_<public_id>.<secret>`
  * Store only:

    * `public_id` (indexed lookup)
    * `argon2(secret)` (verify)
* **AuthZ**:

  * API key is granted access to a set of gateway model aliases (and later tags/teams/budgets).
  * On request:

    1. authenticate key
    2. authorize requested model (alias or tag query)
    3. route to provider target (provider credentials are held by the gateway)

**Example request (non-streaming)**

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer gwk_abcd1234.XYZ..." \
  -H "Content-Type: application/json" \
  -d '{
    "model": "fast",
    "messages": [{"role":"user","content":"ping"}],
    "temperature": 0.2
  }'
```

---

#### Canonical schema + normalization

* Canonical request/response schema for MVP: **OpenAI Chat Completions + Embeddings**
* Internally:

  * Parse into a typed core struct (for validation and routing) but retain unknown fields via `#[serde(flatten)] extra: Map<String, Value>` for safe pass-through to OpenAI-compatible providers.
* Error normalization:

  * Always return an OpenAI-style error envelope (even if upstream differs), e.g.:

```json
{
  "error": {
    "type": "gateway_error",
    "code": "upstream_timeout",
    "message": "Upstream provider timed out",
    "param": null
  }
}
```

---

#### Routing (minimal but extensible)

**Core rule**: `request.model` refers to a **gateway model alias**, not a provider model string.

* Each gateway model has an ordered list of **routes** (“targets”):

  * `(provider_id, upstream_model, priority, weight, extra_headers/body)`
* Router algorithm (MVP):

  1. Select the gateway model by `model`:

     * If exact alias exists → use it
     * Else if `model` matches `tag:<tag1>,<tag2>` → pick best match among allowed models (see below)
  2. Expand to its route list
  3. Try routes in ascending `priority`

     * For routes with same `priority`, pick by `weight` (simple weighted random)
  4. On retryable failures, try next route (fallback)

**Tagging / grouping**

* Gateway models have tags like `cheap`, `fast`, `reasoning`
* Client may request:

  * `model: "tag:fast"` (single tag)
  * `model: "tag:fast,cheap"` (AND semantics)
* Selection rule (MVP):

  * Filter to models the key can access
  * Prefer models with:

    1. all requested tags
    2. lowest `rank` (admin-defined integer; default 100)
    3. then deterministic tie-break by alias

This gives a workable “group routing” without building a full policy DSL.

---

#### Proxy execution: timeouts, retries, idempotency

* **Timeout defaults**

  * Connect: 3s
  * Non-streaming total: 120s (configurable)
  * Streaming:

    * “time-to-first-token” soft timeout: 15s
    * total: 10m upper bound (configurable)

* **Retries**

  * Safe default: **no automatic retries** once upstream request is known to have been accepted (because completions are not strictly idempotent).
  * Allow retry/fallback only on:

    * connection failures before request body is fully sent
    * explicit upstream transient errors (429/5xx/408) *and only if* caller provided an idempotency key (see below)
  * Backoff:

    * exponential with jitter
    * cap at ~2s in MVP

* **Idempotency**

  * Accept `Idempotency-Key` and pass through to upstream providers that support it (OpenAI-compatible providers often do).
  * Add gateway-generated `X-Request-ID` if absent; return it.

---

#### Streaming (SSE proxying)

* **Client interface**: OpenAI-style SSE stream

  * `Content-Type: text/event-stream`
  * `data: {json}\n\n` chunks
  * terminal `data: [DONE]\n\n`

* **Implementation approach**

  * Represent upstream stream as a `Stream<Item = Result<Bytes, ProviderError>>`
  * For OpenAI-compatible upstream SSE:

    * prefer **byte-pass-through** (minimal overhead) unless you need to rewrite chunk fields
  * For non-SSE upstreams (e.g., provider-specific streaming):

    * transform into OpenAI chunk events in the provider adapter layer

* **Backpressure**

  * Use Axum/Hyper body streaming; do not buffer entire streams.
  * If client disconnects:

    * cancel upstream request (drop body, abort task)
    * emit a structured log with `disconnect=true`

---

### 4) Configuration & Data Model

#### Config-as-code format

**Recommendation: YAML** (readable, matches LiteLLM/Helicone patterns), with JSON support later via serde.

Key design choice: support **secret references** explicitly (copy Bifrost’s `env.` style; avoid LiteLLM’s `os.environ/` Pythonism).

##### Example `gateway.yaml`

```yaml
server:
  bind: "127.0.0.1:8080"
  # default DB for local-first:
  db_url: "sqlite:///./gateway.db"
  log_format: "json" # or "pretty"

auth:
  enabled: true
  # MVP: seed initial keys from config (CLI can also create keys and store in DB)
  api_keys:
    - name: "dev"
      value: "env.GW_DEV_KEY"     # or "literal.gwk_xxx.yyy" (discouraged)
      allowed_models: ["fast", "reasoning"]

providers:
  - id: "openai-prod"
    type: "openai_compat"
    base_url: "https://api.openai.com/v1"
    auth:
      kind: "bearer"
      token: "env.OPENAI_API_KEY"
    default_headers:
      # optional:
      OpenAI-Organization: "env.OPENAI_ORG"

  - id: "openrouter"
    type: "openai_compat"
    base_url: "https://openrouter.ai/api/v1"
    auth:
      kind: "bearer"
      token: "env.OPENROUTER_API_KEY"
    default_headers:
      HTTP-Referer: "https://example.local"
      X-Title: "Gateway MVP"

  - id: "vertex-gemini"
    type: "gcp_vertex"
    project_id: "my-gcp-project"
    location: "us-central1"
    auth:
      kind: "service_account"
      credentials_path: "env.GCP_VERTEX_CREDENTIALS_PATH"
    # optional provider defaults:
    timeouts:
      total_ms: 120000

models:
  - id: "fast"
    description: "Cheap/fast chat"
    tags: ["fast", "cheap"]
    rank: 10
    routes:
      - provider: "openrouter"
        upstream_model: "google/gemini-2.0-flash"
        priority: 10
        weight: 1.0
      - provider: "vertex-gemini"
        upstream_model: "gemini-2.0-flash"
        priority: 20
        weight: 1.0

  - id: "reasoning"
    description: "Higher reasoning quality"
    tags: ["reasoning"]
    rank: 20
    routes:
      - provider: "openai-prod"
        upstream_model: "o3-mini"
        priority: 10
        weight: 1.0

routing:
  # MVP: minimal routing knobs; advanced DSL later
  retry_on_status: [408, 429, 500, 502, 503, 504]
  max_attempts_per_request: 2 # includes primary attempt + 1 fallback
```

##### Config validation rules (MVP)

* Provider IDs unique; model IDs unique
* Every route references an existing provider
* If `auth.enabled=true`, at least one API key exists (or CLI-managed DB has keys)
* Secret refs:

  * `env.VAR` must exist unless `--allow-missing-env` is set (useful for CI validation)

---

#### Database schema (Postgres)

> Note: use the same logical schema in SQLite with compatible types (store arrays as JSON text if needed).

**Providers**

* Store **non-secret config** separately from secrets, to reduce accidental leakage in logs/dumps.
* Prefer storing **secret references** (env var names) for local usage; allow encrypted literal secrets as an option.

```sql
-- Enable UUID generation
CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE providers (
  id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  provider_key TEXT UNIQUE NOT NULL,               -- stable string id, e.g. "openai-prod"
  provider_type TEXT NOT NULL,                      -- "openai_compat" | "gcp_vertex" | ...
  config       JSONB NOT NULL,                      -- non-secret config (base_url, region, etc.)
  secrets      JSONB,                               -- encrypted values or env refs
  created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

**Gateway models + routes**

```sql
CREATE TABLE gateway_models (
  id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  model_key   TEXT UNIQUE NOT NULL,                -- stable alias, e.g. "fast"
  description TEXT,
  tags        TEXT[] NOT NULL DEFAULT '{}',
  rank        INT NOT NULL DEFAULT 100,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE model_routes (
  id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  model_id         UUID NOT NULL REFERENCES gateway_models(id) ON DELETE CASCADE,
  provider_id      UUID NOT NULL REFERENCES providers(id) ON DELETE RESTRICT,
  upstream_model   TEXT NOT NULL,
  priority         INT NOT NULL DEFAULT 100,
  weight           DOUBLE PRECISION NOT NULL DEFAULT 1.0,
  enabled          BOOLEAN NOT NULL DEFAULT TRUE,
  extra_headers    JSONB NOT NULL DEFAULT '{}'::jsonb,
  extra_body       JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX model_routes_model_priority_idx
  ON model_routes (model_id, priority);
```

**Users (stub) + API keys (MVP)**

```sql
CREATE TABLE users (
  id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  email        TEXT UNIQUE,
  display_name TEXT,
  status       TEXT NOT NULL DEFAULT 'active',
  created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE api_keys (
  id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id     UUID REFERENCES users(id),
  public_id   TEXT UNIQUE NOT NULL,        -- "abcd1234" part of gwk_abcd1234.secret
  secret_hash TEXT NOT NULL,               -- argon2 hash of secret
  name        TEXT,
  status      TEXT NOT NULL DEFAULT 'active',
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  last_used_at TIMESTAMPTZ,
  revoked_at  TIMESTAMPTZ
);

CREATE TABLE api_key_model_grants (
  api_key_id UUID NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
  model_id   UUID NOT NULL REFERENCES gateway_models(id) ON DELETE CASCADE,
  PRIMARY KEY (api_key_id, model_id)
);
```

**Audit logs (optional MVP)**

```sql
CREATE TABLE audit_logs (
  id        BIGSERIAL PRIMARY KEY,
  ts        TIMESTAMPTZ NOT NULL DEFAULT now(),
  actor_api_key_id UUID,
  action    TEXT NOT NULL,
  object_type TEXT NOT NULL,
  object_id  TEXT,
  details   JSONB NOT NULL DEFAULT '{}'::jsonb
);
```

---

#### DB abstraction for Postgres + Turso/SQLite

**Goal**: one domain-level interface; multiple storage backends.

Minimal trait outline:

```rust
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ApiKeyRecord {
    pub id: Uuid,
    pub public_id: String,
    pub secret_hash: String,
    pub status: String,
    pub user_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct GatewayModelRecord {
    pub id: Uuid,
    pub model_key: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub rank: i32,
}

#[derive(Debug, Clone)]
pub struct ModelRouteRecord {
    pub provider_key: String,
    pub upstream_model: String,
    pub priority: i32,
    pub weight: f64,
    pub extra_headers: Value,
    pub extra_body: Value,
}

#[async_trait]
pub trait GatewayStore: Send + Sync {
    // Auth
    async fn get_api_key_by_public_id(&self, public_id: &str) -> anyhow::Result<Option<ApiKeyRecord>>;
    async fn touch_api_key_last_used(&self, api_key_id: Uuid) -> anyhow::Result<()>;
    async fn list_granted_models(&self, api_key_id: Uuid) -> anyhow::Result<Vec<String>>; // model_key list

    // Model registry
    async fn get_model_by_key(&self, model_key: &str) -> anyhow::Result<Option<GatewayModelRecord>>;
    async fn list_models_for_key(&self, api_key_id: Uuid) -> anyhow::Result<Vec<GatewayModelRecord>>;
    async fn list_routes_for_model(&self, model_id: Uuid) -> anyhow::Result<Vec<ModelRouteRecord>>;

    // Provider registry
    async fn get_provider_config(&self, provider_key: &str) -> anyhow::Result<Option<Value>>; // typed later

    // Config sync (used at startup)
    async fn upsert_provider(&self, provider_key: &str, provider_type: &str, config: Value, secrets: Option<Value>) -> anyhow::Result<()>;
    async fn upsert_model_with_routes(&self, model: GatewayModelRecord, routes: Vec<ModelRouteRecord>) -> anyhow::Result<()>;
}
```

Backend strategy options:

* **Option A (lowest duplication)**: one `sqlx::AnyPool` implementation supporting Postgres + SQLite; store JSON as text/JSON consistently.
* **Option B (cleaner types)**: two store impls (`PostgresStore`, `SqliteStore`) sharing:

  * migrations + domain structs
  * identical method signatures
  * minimal query duplication via helper functions

Turso later:

* Add `TursoStore` using `libsql` (still implementing `GatewayStore`).

---

### 5) Rust Architecture Proposal (single binary)

#### High-level component diagram (Mermaid)

```mermaid
flowchart LR
  subgraph Client
    C[OpenAI SDK / HTTP client]
  end

  subgraph Gateway["Rust Gateway (single binary)"]
    HTTP[axum HTTP server]
    MW[tower middleware: request-id, auth, limits later]
    H[OpenAI-compatible handlers]
    R[Router policy]
    A[AuthN/AuthZ]
    P[Provider adapters]
    S[(Store: Postgres/SQLite)]
    CFG[Config loader + validator]
    OBS[tracing logs + spans]
  end

  subgraph Providers
    OAI[OpenAI-compatible provider]
    OR[OpenRouter (OpenAI-compat)]
    VTX[GCP Vertex AI]
    CUST[Custom base_url (OpenAI-compat)]
  end

  C --> HTTP --> MW --> H
  H --> A --> S
  H --> R --> S
  R --> P
  P --> OAI
  P --> OR
  P --> VTX
  P --> CUST

  CFG --> S
  HTTP --> OBS
  P --> OBS
  R --> OBS
  A --> OBS
```

---

#### Modules / crates layout

Single workspace, one shipped binary:

* `crates/gateway` (binary)

  * `main.rs` (CLI entry)
  * `cli/` (commands: `serve`, `validate`, `migrate`, `keys`)
  * `http/` (axum router + handlers)
  * `config/` (YAML loader, env interpolation, validation)
  * `domain/` (core types: Provider, Model, Route, Auth)
  * `auth/` (api key parsing, hashing verify, grants)
  * `router/` (route selection policy)
  * `providers/`

    * `mod.rs` registry
    * `openai_compat.rs`
    * `gcp_vertex.rs`
    * `streaming.rs` (shared helpers)
  * `store/`

    * `trait.rs` (`GatewayStore`)
    * `postgres.rs`
    * `sqlite.rs`
    * `migrations/`
  * `observability/` (tracing init, redaction helpers)
  * `error.rs` (thiserror enums + mapping to OpenAI error format)

---

#### Key traits / interfaces

**Provider adapter**

```rust
#[async_trait::async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn provider_type(&self) -> &'static str;

    async fn chat_completions(
        &self,
        req: ProviderChatRequest,
        ctx: ProviderRequestContext,
    ) -> Result<ProviderChatResponse, ProviderError>;

    async fn chat_completions_stream(
        &self,
        req: ProviderChatRequest,
        ctx: ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError>;

    async fn embeddings(
        &self,
        req: ProviderEmbeddingsRequest,
        ctx: ProviderRequestContext,
    ) -> Result<ProviderEmbeddingsResponse, ProviderError>;

    async fn list_models(&self, ctx: ProviderRequestContext)
        -> Result<Vec<UpstreamModelInfo>, ProviderError>; // optional MVP
}
```

**Router**

```rust
pub trait RouterPolicy: Send + Sync {
    fn select_routes(&self, model: &GatewayModelRecord, routes: &[ModelRouteRecord]) -> Vec<ModelRouteRecord>;
    fn should_fallback(&self, err: &ProviderError, attempt: usize) -> bool;
}
```

**AuthZ**

* `Authenticator`:

  * parse/verify key
* `Authorizer`:

  * check `model_key ∈ grants`
  * later: rate limits, budgets, teams

---

#### Error handling strategy

* `thiserror` for domain errors:

  * `ConfigError`, `AuthError`, `RouterError`, `ProviderError`, `StoreError`
* `anyhow` at boundaries:

  * CLI entrypoints (`main`)
  * tests/utilities
* Map errors to:

  * OpenAI error envelope for HTTP responses
  * structured logs with stable fields (`error_code`, `provider`, `model_key`, `attempt`)

---

#### Observability baseline

* Use `tracing` spans at:

  * request ingress
  * auth
  * routing selection
  * each upstream attempt
  * streaming lifecycle (connect, first byte, done, client disconnect)
* Key fields to include:

  * `request_id`
  * `api_key_public_id` (never log secret)
  * `user_id` (optional)
  * `model_key`
  * `provider_key`
  * `upstream_model`
  * `attempt`
  * `status_code`
  * `latency_ms`, `ttft_ms` (if streaming)
* Redaction utilities (see Section 6)
* OTel later:

  * keep spans semantically compatible with OpenTelemetry GenAI conventions (TensorZero explicitly references this; you can align without implementing export yet)

---

### 6) Security & Compliance Baseline (practical)

#### Secret handling

* **MVP policy**

  * Prefer `env.*` references in config for provider keys.
  * Allow `literal.*` secrets only in local dev, with warnings.
* **At-rest strategy**

  * If secrets must be persisted in DB:

    * store encrypted blob in `providers.secrets`
    * key from `GATEWAY_KMS_KEY` (env) or local file
    * use envelope encryption (AES-GCM) with key rotation story later
  * Copy the operational idea from LiteLLM’s “salt key encrypt/decrypt” pattern (but implement in Rust).
* **Never**

  * Write provider keys into logs.
  * Echo provider keys in config validation output.

#### Tenant boundaries (local-first, future-hosted compatible)

* MVP is single-tenant, but enforce boundaries using **API key grants**:

  * a key can only access explicitly granted gateway models.
* Design to add later without rewrites:

  * include nullable `team_id` in `users` and `api_keys`
  * scope all policy checks on `(team_id, user_id, api_key_id)`

#### Logging redaction strategy

* Default logging: **metadata-only**

  * No prompt/response bodies.
* “Debug mode” (explicit opt-in):

  * allow truncated bodies (e.g., first N chars) with strong redaction
* Redact:

  * `Authorization`, `Proxy-Authorization`, `Cookie`, `Set-Cookie`
  * provider key headers
  * any config-defined secret header keys
* Ensure response logging (later UI) supports:

  * selective field redaction
  * hashing/tokenization for PII fields if needed

#### SSRF / egress control (custom base URLs)

Custom `base_url` is powerful and risky.

MVP controls:

* Only allow `http`/`https` schemes.
* Default deny:

  * localhost / private IP ranges / link-local
* Allow override via config:

  * `security.allow_private_networks: true` (explicit)
* Resolve host → IP and check range before request
* Consider DNS rebinding:

  * re-resolve per request (costly) or cache with low TTL; document tradeoff

---

### 7) Delivery Plan

#### Week 1 — Core skeleton + config + model registry

**Deliverables**

* Rust binary with CLI:

  * `gateway serve --config gateway.yaml`
  * `gateway validate --config gateway.yaml`
  * `gateway migrate --db ...`
* Config parsing + validation + env interpolation
* DB layer:

  * SQLite backend working end-to-end (migrations + CRUD)
  * Postgres backend scaffolded behind `GatewayStore`
* Read-only model registry:

  * `GET /v1/models` lists allowed models for the API key
* Auth:

  * API key parsing + hashing + grant enforcement

**Exit criteria**

* Can start gateway locally, authenticate, list models.

---

#### Week 2 — Proxy execution + OpenAI-compat provider adapter

**Deliverables**

* `POST /v1/chat/completions` non-streaming
* `POST /v1/embeddings`
* Provider adapter: `openai_compat`

  * configurable base_url + headers + bearer token
* Router MVP:

  * select routes by alias
  * fallback on retryable errors
* Safe timeouts + minimal retries
* Structured tracing logs with request_id, model_key, provider_key, attempt

**Exit criteria**

* End-to-end: client → gateway → OpenAI-compatible upstream.

---

#### Week 3 — Streaming + Vertex adapter + hardening

**Deliverables**

* Streaming SSE proxy for `/v1/chat/completions`

  * pass-through for OpenAI-compatible SSE
  * consistent termination + disconnect handling
* Vertex adapter (initial scope):

  * Gemini chat and embeddings (as supported)
  * service account auth from credentials path
* Operational hardening:

  * config seeding/upsert into DB (idempotent)
  * better error mapping and retry guards
  * `/healthz` and `/readyz`

**Exit criteria**

* Streaming works reliably; Vertex requests route via saved gateway models.

---

#### Prioritized backlog

**MVP now**

* OpenAI-compatible endpoints
* Providers: OpenAI-compatible + Vertex + custom base_url OpenAI-compatible
* Persistent model registry (SQLite default, Postgres supported)
* API key auth + model grants
* Streaming + non-streaming
* Config validation + structured logs

**Later**

* Team/user management + SSO/OIDC
* Rate limits + budgets (user/team/model/global)
* OTel export + Prometheus metrics endpoint
* Request/response log UI
* Cost model catalog + `/v1/models` enrichment + cost estimation
* BYOK support (forward user keys safely)
* Plugins / policy DSL
* Admin HTTP API (create keys, rotate keys, manage models) + UI

---

#### Risks + mitigations

* **Streaming differences across providers**

  * Mitigation: abstract streaming in provider adapters; add golden tests per provider; prefer pass-through where formats match.
* **Non-idempotent retries causing double billing**

  * Mitigation: default to no retries after send; require `Idempotency-Key` to enable certain retries; cap attempts.
* **Provider quirks / parameter drift**

  * Mitigation: typed core + `extra` pass-through; provider-specific allowlists; document compatibility.
* **Token/cost accounting is hard**

  * Mitigation: postpone cost tooling; log enough metadata now (model, provider, usage fields if returned) to backfill later.
* **SSRF via custom base URLs**

  * Mitigation: strong URL validation + private-network deny by default; explicit allow overrides.

---

## Sources

* LiteLLM Proxy — Docker quick start tutorial (model_list, master_key, salt key encryption, virtual keys) ([LiteLLM][1])

* LiteLLM Proxy — Configs overview (config.yaml sections, model aliasing, router settings, fallbacks, retries/timeouts) ([LiteLLM][2])

* LiteLLM Proxy — Custom Auth (override default auth, UserAPIKeyAuth fields) ([LiteLLM][3])

* Helicone AI Gateway — GitHub README (self-host steps, routers YAML snippet, env keys, base URL paths) ([GitHub][4])

* Helicone AI Gateway — Provider routing docs (model registry routing priority, cheapest-first, failover triggers, model-string routing syntax) ([Helicone][5])

* Helicone AI Gateway — Embedded providers/models list (providers.yaml raw) ([GitHub][6])

* Helicone AI Gateway — Overview page (OpenAI-compatible unified API framing) ([Helicone][7])

* Bifrost Gateway — Provider configuration (providers map, keys weights, model-specific keys, network_config with retries/base_url, concurrency tuning, proxies, raw request/response toggles) ([docs.getbifrost.ai][8])

* Bifrost Gateway — Request options (x-bf-* headers: virtual key, api key selection, send raw response, passthrough extra params, extra headers) ([docs.getbifrost.ai][9])

* Bifrost Gateway — Built-in observability (logs store SQLite/Postgres, stats, websocket updates) ([docs.getbifrost.ai][10])

* Bifrost Gateway — Custom providers (multiple instances, allowed request types, config shape) ([docs.getbifrost.ai][11])

* TensorZero Gateway — Overview (scope, provider breadth, access controls, GitOps config, built-in observability) ([tensorzero.com][12])

* TensorZero Gateway — Deploy gateway (CLI args, env vars for provider creds, status/health endpoints) ([tensorzero.com][13])

* TensorZero Gateway — Configure models & providers (models.<name> routing list, provider blocks, fallback routing) ([tensorzero.com][14])

* TensorZero Gateway — Configuration reference (auth caching, timeouts, OTel export knobs, models/providers schema) ([tensorzero.com][15])

[1]: https://docs.litellm.ai/docs/proxy/docker_quick_start "https://docs.litellm.ai/docs/proxy/docker_quick_start"
[2]: https://docs.litellm.ai/docs/proxy/configs "https://docs.litellm.ai/docs/proxy/configs"
[3]: https://docs.litellm.ai/docs/proxy/custom_auth "https://docs.litellm.ai/docs/proxy/custom_auth"
[4]: https://github.com/Helicone/ai-gateway "https://github.com/Helicone/ai-gateway"
[5]: https://docs.helicone.ai/gateway/provider-routing "https://docs.helicone.ai/gateway/provider-routing"
[6]: https://raw.githubusercontent.com/Helicone/ai-gateway/main/ai-gateway/config/embedded/providers.yaml "https://raw.githubusercontent.com/Helicone/ai-gateway/main/ai-gateway/config/embedded/providers.yaml"
[7]: https://docs.helicone.ai/gateway/overview "https://docs.helicone.ai/gateway/overview"
[8]: https://docs.getbifrost.ai/quickstart/gateway/provider-configuration "https://docs.getbifrost.ai/quickstart/gateway/provider-configuration"
[9]: https://docs.getbifrost.ai/providers/request-options "https://docs.getbifrost.ai/providers/request-options"
[10]: https://docs.getbifrost.ai/features/observability/default "https://docs.getbifrost.ai/features/observability/default"
[11]: https://docs.getbifrost.ai/providers/custom-providers "https://docs.getbifrost.ai/providers/custom-providers"
[12]: https://www.tensorzero.com/docs/gateway "https://www.tensorzero.com/docs/gateway"
[13]: https://www.tensorzero.com/docs/deployment/tensorzero-gateway "https://www.tensorzero.com/docs/deployment/tensorzero-gateway"
[14]: https://www.tensorzero.com/docs/gateway/configure-models-and-providers "https://www.tensorzero.com/docs/gateway/configure-models-and-providers"
[15]: https://www.tensorzero.com/docs/gateway/configuration-reference "https://www.tensorzero.com/docs/gateway/configuration-reference"

