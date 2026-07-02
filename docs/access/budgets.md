# Budgets

`See also`: [Service Accounts](service-accounts.md), [MCP Tool Access](mcp-tool-access.md), [Admin Control Plane](admin-control-plane.md), [Budgets and Spending](../operations/budgets-and-spending.md), [Data Relationships](../reference/data-relationships.md)

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
