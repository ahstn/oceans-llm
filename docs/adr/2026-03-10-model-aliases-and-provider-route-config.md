# ADR: Model Aliases and Provider-Only Route Config

- Date: 2026-03-10
- Status: Accepted

## Context

The gateway already separates two concepts:

- `gateway_models` are the client-facing model identifiers and grant targets.
- `model_routes` are concrete provider execution targets.

Issue #39 introduced a new requirement: keep deprecated model keys callable while transparently routing execution to a replacement model. At the same time, we needed to clarify how multiple providers should be represented for one model without introducing a second routing policy layer.

## Decision

### 1. Aliasing lives on the model, not in `routes`

We model indirection with `alias_of` in config and `alias_target_model_id` in storage.

Why:
- aliasing changes which gateway model is canonical, not which provider route entry to pick,
- grants and `/v1/models` already operate on gateway model records,
- keeping aliasing out of `routes` preserves the meaning of `model_routes` as provider execution targets only.

### 2. A model is either alias-backed or provider-backed

One model cannot define both `alias_of` and `routes`.

Why:
- hybrid semantics would require precedence rules and more complex validation,
- the current requirement is stable canonicalization of one model key onto another,
- mutual exclusivity keeps seeding, validation, and runtime resolution straightforward.

### 3. Multi-provider routing remains per-route

We keep `priority` and `weight` on each provider route and do not add a top-level `provider_routing` object.

Why:
- the existing planner already supports ordered failover by `priority` and weighted selection within the same priority tier,
- a second routing object would duplicate behavior and make config harder to reason about,
- provider-backed models remain simple YAML lists of concrete execution targets.

### 4. Requested model identity remains the public contract

Runtime resolution is formalized through an explicit model-resolution contract:

- `ResolvedModelSelection` carries the requested gateway model, canonical execution model, and alias chain.
- `ResolvedGatewayRequest` carries auth, the resolved model selection, and the planned provider routes.

Why:
- auth and grants should continue to be evaluated against what the client requested,
- client-visible `response.model` should stay stable across aliases and provider remaps,
- the service layer now has one place to enforce alias traversal, cycle detection, and depth limits before provider planning begins.

### 5. Request logs store requested and resolved model identity separately

Request logs now persist both:

- `model_key`: the requested gateway model key
- `resolved_model_key`: the canonical execution model key

Why:
- aliasing is now a durable routing feature rather than an ad hoc metadata detail,
- typed columns are easier to query and reason about than metadata conventions,
- the public contract stays stable while observability retains the actual execution target.

### 6. Backend parity is a required rule for model-registry features

Alias-related schema, seeding, hydration, and request logging ship on libsql and Postgres together.

Why:
- the runtime now supports both backends in real workflows,
- “libsql first, Postgres later” creates drift in model-registry behavior,
- parity pressure keeps migrations, store modules, and tests aligned as the schema evolves.

## Consequences

Positive:
- deprecated model names can remain live without duplicating provider routes,
- multi-provider config stays backward-compatible,
- the model registry remains the single source of truth for grants and public model names.

Tradeoffs:
- runtime resolution needs defensive cycle and depth checks even though config validation rejects bad alias graphs,
- alias-backed models do not expose their canonical target directly through the public API in this slice,
- request logging gains another persisted field that must be kept in sync across both backends.

## Follow-up Work

- Expose alias/deprecation metadata through admin or public model APIs if needed.
- Revisit richer routing-policy configuration only when weighted-priority routing becomes insufficient.
- Consider whether `resolved_model_key` should become part of downstream spend/accounting tables once usage-cost events are written.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
