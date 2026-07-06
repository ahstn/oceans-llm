# Budgets

`See also`: [Service Accounts](service-accounts.md), [MCP Tool Access](../mcp/mcp-tool-access.md), [Admin Control Plane](admin-control-plane.md), [Budgets and Spending](../contributing/operations/budgets-and-spending.md), [Data Relationships](../contributing/reference/data-relationships.md)

Budgets limit or monitor gateway spend for principals that can generate spend. They are spend controls, not model authorization controls; API-key grants and model access policies decide whether a caller may use a model before budget enforcement runs.

## Taxonomy

Spend-bearing principals are:

- human users
- service accounts

Human users can also have model-specific budgets.

Supported budget types:

- User budget: applies to all spend from one human user.
- Service account budget: applies to all spend from one service account.
- User model budget: applies to one user's spend for one gateway model or, when no gateway model id is available, one exact trimmed upstream model name.

There is no standalone global model budget. Model-specific spend control is scoped to users. Admins can define config defaults that create a user model budget for every user for selected expensive models.

Teams are not budget principals. Teams group users, own service accounts, and provide reporting metadata for service-account spend.

MCP tool grants and toolsets are separate access controls. MCP token-overhead estimates report context-window pressure from tool definitions and results; they are not spend-budget accounting and do not create budget charges.

## Budget Levers

Every budget has the same settings:

- `cadence`: `daily`, `weekly`, or `monthly`
- `amount_usd`: the spend cap, stored with four decimal places
- `hard_limit`: `true` blocks chargeable traffic after the cap is reached; `false` only reports and alerts
- `timezone`: stored with the budget for display and future window behavior

Live enforcement windows currently use UTC:

- daily windows start at `00:00:00 UTC`
- weekly windows start at `Monday 00:00:00 UTC`
- monthly windows start at `00:00:00 UTC` on the first day of the month

## Hard And Soft Limits

Hard budgets reject new chargeable traffic when the active window is already exhausted. If a request starts under the limit but its final priced usage would push the window over the budget, the gateway rejects that completed charge before recording it as spend.

Soft budgets never reject traffic. They are useful for alerting and reporting when a team wants visibility before enforcing a hard cap.

## Overlap Rules

For human user traffic, Oceans checks budgets in this order:

1. matching user model budget
2. user budget

For service-account traffic, Oceans checks only the service-account budget.

If a user has both a user model budget and a user budget, the model-specific budget is evaluated first. Both can still alert independently. Budgets do not grant model access; a request blocked by API-key grants or an allowlist never reaches the budget gate.

User model budgets match the resolved gateway model id when one is available. Use the upstream model fallback only when the gateway cannot attach a model id to the ledger row; it matches the exact trimmed upstream model string.

## Budget Sources

Active budgets can come from:

- admin UI or admin API changes
- `users[*].budget` entries for config-seeded users
- the global default user budget under `budgets.users.default`
- per-model default user budgets under `budgets.users.model_defaults`

Admin UI and admin API edits are manual overrides. Editing an inherited default budget converts that budget to manual, so later config reloads do not overwrite it.

Deactivating an inherited budget through the admin API or UI is also a manual override. The budget remains inactive on later config reloads unless an admin creates a new manual budget or a config-seeded per-user budget explicitly owns that user's budget.

## Configure In The Admin UI

Open `/admin/spend-controls`.

The page has three budget sections:

- User Budgets
- Service Account Budgets
- User Model Budgets

Use User Budgets for normal human access. Choose the user, cadence, amount, timezone, and whether the budget is hard or soft.

Use Service Account Budgets before activating automation credentials. Choose the service account and the same budget controls. Active service-account API keys require this budget.

Use User Model Budgets when one user needs a lower or separate limit for a specific model. Choose the user, then choose either:

- a gateway model from the model selector, when the gateway model id is known
- the exact trimmed upstream model name only for fallback cases where no gateway model id is available

Then set cadence, amount, timezone, and hard-limit behavior.

## Configure With The Admin API

Admins can manage the same budget scopes through `/api/v1/admin/spend/budgets`.

List budgets and current-window spend:

```bash
curl -sS "$OCEANS_BASE_URL/api/v1/admin/spend/budgets" \
  -H "cookie: $OCEANS_ADMIN_SESSION_COOKIE"
```

Budget list and upsert responses include `budget_source` with:

- `kind`: `manual`, `config_user_override`, `config_user_default`, or `config_user_model_default`
- `key`: source-specific metadata, such as the config path or seeded user email

Any `PUT /api/v1/admin/spend/budgets` request writes a manual budget, even when the previous row was inherited from config.

Create or update a user budget:

```bash
curl -sS -X PUT "$OCEANS_BASE_URL/api/v1/admin/spend/budgets" \
  -H "content-type: application/json" \
  -H "cookie: $OCEANS_ADMIN_SESSION_COOKIE" \
  --data '{
    "scope": {
      "kind": "user",
      "user_id": "00000000-0000-0000-0000-000000000000"
    },
    "cadence": "monthly",
    "amount_usd": "100.0000",
    "hard_limit": true,
    "timezone": "UTC"
  }'
```

Create or update a service-account budget:

```bash
curl -sS -X PUT "$OCEANS_BASE_URL/api/v1/admin/spend/budgets" \
  -H "content-type: application/json" \
  -H "cookie: $OCEANS_ADMIN_SESSION_COOKIE" \
  --data '{
    "scope": {
      "kind": "service_account",
      "service_account_id": "00000000-0000-0000-0000-000000000000"
    },
    "cadence": "daily",
    "amount_usd": "25.0000",
    "hard_limit": true,
    "timezone": "UTC"
  }'
```

Create or update a user model budget with the managed gateway model id:

```bash
curl -sS -X PUT "$OCEANS_BASE_URL/api/v1/admin/spend/budgets" \
  -H "content-type: application/json" \
  -H "cookie: $OCEANS_ADMIN_SESSION_COOKIE" \
  --data '{
    "scope": {
      "kind": "user_model",
      "user_id": "00000000-0000-0000-0000-000000000000",
      "model_id": "00000000-0000-0000-0000-000000000000"
    },
    "cadence": "daily",
    "amount_usd": "5.0000",
    "hard_limit": true,
    "timezone": "UTC"
  }'
```

Create or update a user model budget with the upstream-model fallback:

```bash
curl -sS -X PUT "$OCEANS_BASE_URL/api/v1/admin/spend/budgets" \
  -H "content-type: application/json" \
  -H "cookie: $OCEANS_ADMIN_SESSION_COOKIE" \
  --data '{
    "scope": {
      "kind": "user_model",
      "user_id": "00000000-0000-0000-0000-000000000000",
      "upstream_model": "gpt-5"
    },
    "cadence": "daily",
    "amount_usd": "5.0000",
    "hard_limit": true,
    "timezone": "UTC"
  }'
```

Deactivate a budget by posting the same scope:

```bash
curl -sS -X POST "$OCEANS_BASE_URL/api/v1/admin/spend/budgets/deactivate" \
  -H "content-type: application/json" \
  -H "cookie: $OCEANS_ADMIN_SESSION_COOKIE" \
  --data '{
    "scope": {
      "kind": "user_model",
      "user_id": "00000000-0000-0000-0000-000000000000",
      "model_id": "00000000-0000-0000-0000-000000000000"
    }
  }'
```

`model_id` is the gateway model UUID from the admin models API/UI, not the model key that callers send as `model`.

## Configure From YAML

Config-seeded users can include one active user budget:

```yaml
users:
  - name: Platform Admin
    email: ops@example.com
    auth_mode: password
    global_role: platform_admin
    membership:
      team: platform
      role: admin
    budget:
      cadence: monthly
      amount_usd: "100.0000"
      hard_limit: true
      timezone: UTC
```

Omitting `budget` for a listed config-seeded user does not deactivate that user's active budget. Absence inherits the global default user budget when configured, or leaves any existing manual/API state alone.

Set a default user budget for all human users with `budgets.users.default`:

```yaml
budgets:
  users:
    default:
      cadence: daily
      amount_usd: "70.0000"
      hard_limit: true
      timezone: UTC
```

Set default user model budgets for selected gateway models with `budgets.users.model_defaults`:

```yaml
budgets:
  users:
    default:
      cadence: daily
      amount_usd: "70.0000"
      hard_limit: true
      timezone: UTC
    model_defaults:
      - model: fable-5
        budget:
          cadence: daily
          amount_usd: "40.0000"
          hard_limit: true
          timezone: UTC
```

The `model` value is the configured gateway model id from `models[*].id`. These defaults apply to all human users, including config-seeded users, bootstrap admins, admin-created users, and JIT OIDC/OAuth users. They do not apply to service accounts.

Config-seeded `users[*].budget` is a per-user override over `budgets.users.default`. Manual admin/API changes still take precedence over inherited defaults.

Declarative service accounts define their owning team, budget, and managed gateway API keys:

```yaml
service_accounts:
  - id: ci-indexer
    name: CI Indexer
    team: platform
    budget:
      cadence: daily
      amount_usd: "25.0000"
      hard_limit: true
      timezone: UTC
    keys:
      - id: primary
        name: CI Indexer Primary
        value: env.CI_INDEXER_GATEWAY_API_KEY
        allowed_models:
          - fast
```

The owning team must be declared in `teams`. The budget block is required.

User-specific model budget overrides are configured in `/admin/spend-controls` or with `PUT /api/v1/admin/spend/budgets`.

## Monitor Budgets

`/admin/spend-controls` shows:

- each user budget and current-window spend
- each service-account budget and current-window spend
- active user model budgets and current-window spend
- alert recipient readiness
- recent threshold alert delivery status

Budget alert history is also available from `GET /api/v1/admin/spend/budget-alerts`.

Alerts are created when remaining budget crosses to `20%` or less. User and user model budget alerts go to the user's email. Service-account budget alerts go to active owners and admins of the owning team.

Spend reporting and export live outside the budget setup page:

- `GET /api/v1/admin/spend/report`
- `GET /api/v1/admin/spend/focus.csv`
- `GET /api/v1/me/spend/focus.csv`

## Embedding Spend

Embedding requests use the same budget taxonomy as chat and Responses traffic. When a native Vertex embedding request has real provider token usage (`statistics.token_count` for Vertex `:predict` text-embedding models or `usageMetadata.promptTokenCount` for `google/gemini-embedding-2`) and exact pricing, the resulting spend counts toward:

- the caller's user budget for human-owned API keys
- the caller service account's service-account budget for service-account-owned API keys
- a matching user model budget when a human user calls the specific gateway embedding model

Rows that are `unpriced` or `usage_missing` stay visible in spend reporting, but they do not consume hard or soft budget windows. This can happen when a provider returns embeddings without usable token counts or when the pricing catalog does not have an exact price for the selected Vertex embedding model/location.

The Google Cloud service account configured under a Vertex provider's `auth.mode: service_account` is only upstream provider credential material. It is not a gateway spend principal and does not receive a service-account budget. Gateway service-account budgets apply to service accounts created in Oceans for non-human callers.

To give one user a separate cap for an embedding model:

1. Configure a gateway model such as `gemini-embedding` with an embedding-capable route.
2. Open `/admin/spend-controls`.
3. Create a User Model Budget.
4. Select the user.
5. Select the gateway embedding model, for example `gemini-embedding`.
6. Choose cadence, amount, hard-limit behavior, and timezone.

To cap automation that uses embeddings, create or select the gateway service account used by that workload and configure a Service Account Budget before activating its API key.

## Service Account Requirement

Active service-account API keys require an active service-account budget. This is true for keys created in the admin UI and keys seeded from configuration.

Admins cannot deactivate a service-account budget while active API keys exist for that service account. Revoke or deactivate the keys first.
