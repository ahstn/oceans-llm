# ADR: FOCUS Billing Data Export

- Date: 2026-05-19
- Status: Accepted
- Superseded in part by: [2026-05-27 Budget Principal Taxonomy](2026-05-27-budget-principal-taxonomy.md)
- Related Issues:
  - [#159](https://github.com/ahstn/oceans-llm/issues/159)
- Builds On:
  - [2026-03-12-durable-usage-ledger-accounting.md](2026-03-12-durable-usage-ledger-accounting.md)
  - [2026-03-15-spend-control-plane-reporting-and-team-hard-limits.md](2026-03-15-spend-control-plane-reporting-and-team-hard-limits.md)
  - [2026-04-21-observability-usage-leaderboard.md](2026-04-21-observability-usage-leaderboard.md)

## Context

FinOps tooling increasingly supports the FinOps Open Cost and Usage Specification
(FOCUS) as a common billing-data schema. Finout supports custom FOCUS CSV upload
and a native FOCUS table. Vantage supports custom provider imports via CSVs that
follow the FinOps FOCUS schema. The upstream FOCUS project defines versioned
requirements for billing datasets, including standard column ordering and the
use of `x_`-prefixed custom columns after FOCUS columns.

Oceans already records a durable LLM usage ledger in `usage_cost_events`. The
ledger stores per-request usage and pricing facts, including:

- request and ownership scope identifiers,
- API key, user, team, service account, and actor user identifiers when present,
- model/provider/upstream model information,
- prompt, completion, and total token counts,
- pricing status and computed USD cost,
- occurrence timestamp.

The existing admin spend report aggregates this data for operational dashboards,
but customers also need portable billing exports they can import into external
FinOps and cloud-cost management tools.

## Decision

Add a first-party FOCUS export capability for LLM usage costs.

The v1 export is a **FOCUS v1.2-compatible, best-effort CSV**. It should be
suitable for tools that accept FOCUS-like custom provider data, while avoiding a
claim of strict FOCUS certification until every mandatory column and semantic
requirement has been validated against the spec.

### Export grain

The default and only v1 row grain is:

> one row per UTC day, owner scope, upstream provider/model, and pricing status
> for priced or legacy-estimated usage.

This avoids request-level data leakage, keeps CSV size predictable, and aligns
with how FinOps tools typically consume charge data. Raw per-request export can
be added later as an admin-only audit feature if needed.

### Pricing status handling

Only `priced` and `legacy_estimated` ledger rows are exported as standard FOCUS
charge rows.

Rows with `unpriced` or `usage_missing` are excluded from the FOCUS CSV because
FOCUS cost metrics such as `BilledCost` are non-null charge values, and exporting
unknown cost as zero would be misleading. The API should return or make available
a companion diagnostics summary for excluded aggregates so admins can see pricing
coverage gaps.

### Authorization

- Platform admins can export all visible spend scopes.
- Regular users can export only spend attributable to themselves.
- Team-scoped export for team admins can be added when team-admin authorization
  semantics exist in the product.

### Delivery surface

v1 exposes synchronous CSV download:

- an HTTP endpoint that streams or returns `text/csv`,
- an admin UI button on the existing Usage Costs page,
- the same date range and owner-kind controls where applicable.

Scheduled exports, direct Finout/Vantage integrations, object-storage delivery,
and API-token automation are deferred.

### Date ranges

The export accepts explicit inclusive `start` and `end` dates, defaults to the
last 30 complete UTC days excluding the in-progress UTC day, and rejects synchronous exports longer than 90 days. All
aggregation boundaries are UTC.

Daily exports are supported through the same daily row grain by passing
`day=YYYY-MM-DD`. A daily export is equivalent to `start=<day>&end=<day>` and is
converted internally to the exclusive UTC timestamp window `[day 00:00Z, next day
00:00Z)`.

### Provider and allocation semantics

The gateway/application is represented as the billing-data provider. Upstream LLM
provider and model are service/SKU/resource details plus custom `x_` columns.

The owner scope is the primary allocation identity. API key and actor details are
secondary/custom columns where they are safe and available.

## Initial column mapping

The implementation should put standard FOCUS columns first and custom columns
afterwards with the `x_` prefix. Exact mandatory-column coverage must be checked
against the chosen FOCUS v1.2 schema during implementation.

| Export column | Source / value | Notes |
| --- | --- | --- |
| `ProviderName` | configured gateway/provider display name | Represents Oceans/gateway as the billing-data provider. |
| `PublisherName` | same as `ProviderName` unless configured separately | Best-effort SaaS/internal provider mapping. |
| `BillingAccountId` | configured tenant/org id or stable deployment id | Must be stable across exports. |
| `BillingAccountName` | configured tenant/org name | Human-readable billing account. |
| `SubAccountId` | owner id | User/team/service account UUID. |
| `SubAccountName` | owner display name where available | Falls back to owner id. |
| `ChargePeriodStart` | UTC day start | Inclusive. |
| `ChargePeriodEnd` | UTC next-day start | Exclusive. |
| `ChargeCategory` | `Usage` | v1 exports usage charges only. |
| `ChargeClass` | empty/null unless correction support exists | Corrections are not modeled in v1. |
| `BillingCurrency` | `USD` | Current ledger stores computed USD cost. |
| `BilledCost` | summed `computed_cost_10000 / 10000` | For priced and legacy-estimated rows only. |
| `EffectiveCost` | same as `BilledCost` | No separate amortization/commitment modeling in v1. |
| `ListCost` | same as `BilledCost` initially | No independent list-price ledger field in v1. |
| `ContractedCost` | same as `BilledCost` initially | No independent contracted-rate ledger field in v1. |
| `ServiceCategory` | `AI and Machine Learning` or closest allowed value | Must be validated against FOCUS allowed values. |
| `ServiceName` | `LLM Gateway` or upstream provider display | Prefer stable product/service label. |
| `SkuId` | upstream model key | e.g. provider/model identifier. |
| `SkuPriceId` | pricing row id where available | Optional/best-effort. |
| `ConsumedQuantity` | summed total tokens | Token usage quantity. |
| `ConsumedUnit` | `tokens` | Custom/unit compatibility should be validated. |
| `PricingQuantity` | summed total tokens / 1,000,000 | Matches per-million-token pricing basis where possible. |
| `PricingUnit` | `1M tokens` | Mirrors internal pricing model. |
| `RegionName` | empty/null | LLM usage is not currently region-attributed. |
| `ResourceId` | deterministic aggregate id | Derived from date + owner + model + pricing status. |
| `ResourceName` | owner/model summary | Human-readable aggregate resource. |
| `Tags` | owner tags JSON object | User/team tags when present; service-account rows inherit owning team tags because service accounts do not have direct tags. |
| `x_owner_kind` | `user` / `team` / `service_account` | Custom allocation detail. |
| `x_owner_id` | owner UUID | Custom allocation detail. |
| `x_owner_name` | owner display name | Custom allocation detail. |
| `x_upstream_provider` | ledger provider key | Original upstream provider. |
| `x_upstream_model` | ledger upstream model | Original model string. |
| `x_model_id` | internal model id where available | Internal catalog identifier. |
| `x_prompt_tokens` | summed prompt tokens | LLM-specific usage metric. |
| `x_completion_tokens` | summed completion tokens | LLM-specific usage metric. |
| `x_total_tokens` | summed total tokens | LLM-specific usage metric. |
| `x_request_count` | row count | Number of requests in aggregate. |
| `x_pricing_status` | `priced` / `legacy_estimated` | Makes estimated rows auditable. |

## Implementation plan

### Gateway contract and handlers

- Add an admin endpoint such as
  `GET /api/v1/admin/spend/focus.csv?start=YYYY-MM-DD&end=YYYY-MM-DD&owner_kind=all|user|team|service_account`.
- Support daily exports with
  `GET /api/v1/admin/spend/focus.csv?day=YYYY-MM-DD&owner_kind=all|user|team|service_account`.
- Add a user endpoint such as
  `GET /api/v1/me/spend/focus.csv?start=YYYY-MM-DD&end=YYYY-MM-DD` for self-only export.
- Validate date ranges, enforce the 90-day limit, and use UTC day boundaries.
- Return `text/csv` with a download-oriented `Content-Disposition` filename.
- Include diagnostics counts in response headers or expose a companion JSON
  endpoint if headers are insufficient.

### Storage

- Add repository methods for FOCUS export aggregates instead of reusing dashboard
  DTOs. The export needs owner/model/token/cost fields in one shape.
- Implement matching queries for libsql and postgres.
- Filter standard export rows to `pricing_status in ('priced', 'legacy_estimated')`.
- Compute excluded diagnostics for `unpriced` and `usage_missing` rows over the
  same range/filter.

### CSV generation

- Centralize FOCUS column definitions and mapping in a small module to keep
  ordering deterministic.
- Write tests for:
  - header order,
  - CSV escaping,
  - money formatting precision,
  - FOCUS `Tags` JSON rendering from owner/team tags,
  - UTC date boundaries,
  - exclusion of unpriced rows,
  - deterministic aggregate resource ids.

### Admin UI

- Add an "Export FOCUS CSV" action to the Usage Costs page.
- Reuse the selected range/owner filter when possible.
- Surface diagnostics after export if the selected period had excluded unpriced
  or usage-missing rows.

## Tradeoffs

- Daily aggregation reduces detail and improves privacy, but it cannot support
  request-level reconciliation without a later raw export.
- Best-effort FOCUS compatibility is useful sooner, but it requires clear docs to
  avoid implying strict conformance.
- Excluding unpriced rows keeps cost data honest, but admins need diagnostics so
  pricing gaps are not invisible.
- Representing the gateway as provider reflects the billing relationship, but
  users who want upstream-vendor reports must rely on service/SKU/custom columns.
- Synchronous CSV is simple, but large historical exports will require a future
  asynchronous job model.

## Follow-up work

- Strict FOCUS v1.2 validation and documented conformance gaps.
- Tool-specific presets or examples for Finout and Vantage imports.
- Scheduled exports to object storage or email.
- Raw admin-only per-request export for audit/reconciliation.
- Team-admin export once team-admin authorization is defined.
- Multi-currency support if the ledger ever records non-USD billing data.
- Move FOCUS export domain records out of `gateway-core/src/domain.rs` into a
  cohesive domain submodule when the broader domain module is split. They remain
  colocated with existing spend aggregate records in this slice to preserve the
  current public API and avoid a mixed feature/refactor change.

This ADR was prepared through collaborative human + AI discovery, including
FOCUS ecosystem research and guided product decision review.
