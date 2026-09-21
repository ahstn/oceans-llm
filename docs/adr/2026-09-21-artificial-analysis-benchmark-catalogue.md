# ADR: Artificial Analysis benchmark catalogue

- Date: 2026-09-21
- Status: Accepted

## Context

The Models page shows price, limits, routes, and runtime capability metadata. It does not show an independent capability score. Artificial Analysis offers this data through an authenticated V2 API, but its metrics have different units and licence terms.

Gateway models can also be aliases or use several upstream routes. A name or route match can attach a score to the wrong evaluated variant.

## Decision

### 1. The first rollout is internal and Free-tier only

The gateway calls `/api/v2/language/models/free` and stores only the Artificial Analysis Intelligence Index. It does not request or display Pro-only individual evaluations such as Terminal-Bench.

Customer-facing use is disabled. Enabling it needs written commercial rights and a separate decision. Artificial Analysis attribution is visible wherever a score appears.

If API access ends or the applicable terms require deletion, operators must remove stored Artificial Analysis data within 30 days.

### 2. Model identity is explicit

Each scored gateway model sets `artificial_analysis_model_id` to the stable UUID returned by Artificial Analysis. The gateway does not infer a binding from a name, slug, alias, provider route, or OpenRouter ID.

The explicit binding confirms that the operator reviewed the exact evaluated variant. Scores do not pass through aliases unless that alias has its own binding.

### 3. Scores are typed current state

The store keeps:

- model-to-source bindings
- current approved score rows
- the last successful source version and refresh time

Each score has a numeric value, unit, metric key, label, benchmark version, source, source model ID, source URL, and fetch time. The store does not keep the full source catalogue or unbounded history.

### 4. Refresh is separate from pricing

Benchmark refresh uses its own `benchmark_catalog` service module and repository trait. It does not share pricing policy or the 15-minute pricing schedule.

The gateway fetches all API pages, validates the complete response, projects only explicitly bound models, and replaces current scores in one transaction. A successful refresh removes missing or `null` scores. Any fetch or validation failure leaves the last successful set unchanged.

The normal interval is 24 hours. A platform-admin endpoint provides a manual refresh. Model list reads use stored data and never call Artificial Analysis.

### 5. The admin API owns display metadata

`GET /api/v1/admin/models` returns benchmark scores to the admin UI. The OpenAI-compatible `/v1/models` response does not change. The server API key never enters an admin response.

## Consequences

Benefits:

- score identity is auditable and cannot drift through fuzzy matching
- model list availability does not depend on Artificial Analysis
- numeric values keep their scale and can be sorted or formatted safely
- failed refreshes do not erase valid data
- pricing and benchmark rules remain separate

Trade-offs:

- every scored model needs an operator-managed UUID
- the Free tier provides no Terminal-Bench value
- stored scores can become stale until the next successful refresh
- customer-facing display needs a later commercial-rights review

## Follow-up work

- Add another metric only after its API tier and display rights are approved.
- Revisit a separate `gateway-benchmarks` crate only after a second source or consumer creates a stable shared boundary.

## Attribution

This ADR was prepared through collaborative human + AI implementation and design work.
