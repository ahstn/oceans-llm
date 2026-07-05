# Budgets

`See also`: [Service Accounts](service-accounts.md), [MCP Tool Access](../mcp/mcp-tool-access.md), [Admin Control Plane](admin-control-plane.md), [Budgets and Spending](../contributing/operations/budgets-and-spending.md), [Data Relationships](../contributing/reference/data-relationships.md)

Budgets limit or monitor gateway spend for principals that can generate spend.

## Budget Taxonomy

Spend-bearing principals are:

- human users
- service accounts

Human users can also have model-specific budgets.

Supported budget types:

- User budget: applies to all spend from one human user.
- Service account budget: applies to all spend from one service account.
- User model budget: applies to one user's spend for one gateway model or, when no gateway model id is available, one exact trimmed upstream model name.

Teams are not budget principals. Teams group users, own service accounts, and provide reporting metadata for service-account spend.

MCP tool grants and toolsets are separate access controls. MCP token-overhead estimates report context-window pressure from tool definitions and results; they are not spend-budget accounting and do not create budget charges.

## Hard And Soft Budgets

Hard budgets reject new chargeable traffic when the active window is already exhausted or when the completed request would push spend past the budget.

Soft budgets never reject traffic. They are useful for alerting and reporting.

## Overlap Rules

For human user traffic, Oceans checks budgets in this order:

1. matching user model budget
2. user budget

For service-account traffic, Oceans checks only the service-account budget.

If a user has both a user model budget and a user budget, the model-specific budget is evaluated first. Both can still alert independently.

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

## Admin UI Setup

Open `/admin/spend-controls`.

The page has three budget sections:

- User Budgets
- Service Account Budgets
- User Model Budgets

Use User Budgets for normal human access. Use Service Account Budgets before activating automation credentials. Use User Model Budgets when one user needs a lower or separate limit for a specific model.

To set a user budget, choose the user, cadence, amount, and whether the budget is hard or soft. To set a service-account budget, choose the service account and the same budget controls; active service-account API keys require this budget. To set a user model budget, choose the user and model selector, then set the cadence, amount, and hard-limit behavior.

## Config-Seeded Service Accounts

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
