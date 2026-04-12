# ADR: Redacted Provider Credentials in Resolved Request Caches

- Date: 2026-04-12
- Status: Accepted
- Builds On:
  - [2026-03-15-v1-runtime-simplification.md](2026-03-15-v1-runtime-simplification.md)
  - [2026-03-31-chat-observability-hardening-and-stream-parse-contract.md](2026-03-31-chat-observability-hardening-and-stream-parse-contract.md)

## Context

`ResolvedGatewayRequest` is the in-flight runtime object used after model+route resolution and before provider execution. It previously cached full `ProviderConnection` values keyed by `provider_key`.

A full `ProviderConnection` includes `secrets`, and those values can contain raw API keys, service-account private keys, and other sensitive credential material. Keeping that full object in the resolved-request cache increases accidental exposure risk (debug dumps, panic payloads, future logging mistakes) even when the normal execution path does not explicitly log secrets.

At the same time, we still need:

- current request-log icon inference behavior,
- enough provider/credential shape for future admin UX work,
- no change to route selection/provider execution behavior.

## Decision

Replace the resolved-request provider cache with a redacted snapshot type that never stores raw secrets.

Specifically:

1. `ResolvedGatewayRequest.provider_connections` now stores `ResolvedProviderConnection` (not `ProviderConnection`).
2. `ResolvedProviderConnection` keeps only:
   - `provider_key`,
   - `provider_type`,
   - `config`,
   - `redacted_secrets`.
3. `redacted_secrets` preserves JSON structure (objects/arrays/keys) while masking every non-null scalar leaf as `"********"`.
4. Icon metadata resolution uses provider key/type/config via `resolve_provider_display_from_parts(...)`, preserving existing icon behavior without needing full provider secrets.

## How It Was Implemented

- Added `ResolvedProviderConnection` in [../../crates/gateway-service/src/model_resolution.rs](../../crates/gateway-service/src/model_resolution.rs).
- Updated [../../crates/gateway-service/src/service.rs](../../crates/gateway-service/src/service.rs) to cache `ResolvedProviderConnection::from_provider_connection(...)` during request resolution.
- Added `mask_secret_leaf_values(...)` in [../../crates/gateway-service/src/redaction.rs](../../crates/gateway-service/src/redaction.rs) to preserve shape while masking secret leaf values.
- Added `resolve_provider_display_from_parts(...)` in [../../crates/gateway-service/src/icon_identity.rs](../../crates/gateway-service/src/icon_identity.rs), then used it from [../../crates/gateway/src/http/handlers.rs](../../crates/gateway/src/http/handlers.rs) for request-log icon metadata.
- Exported the new public items from [../../crates/gateway-service/src/lib.rs](../../crates/gateway-service/src/lib.rs).

## Rationale

This creates a stricter boundary:

- provider execution still obtains full provider credentials from the provider store/runtime provider client path,
- resolved request metadata only carries what routing/logging/admin-shape features need,
- raw credentials are intentionally excluded from this in-memory request object.

By masking every secret leaf value instead of dropping the whole payload, we retain the shape needed for future UX and diagnostics (e.g., service-account JSON structure) without retaining credential content.

## Consequences and Trade-offs

### Positive

- Reduces blast radius of accidental exposure from resolved-request structures.
- Preserves credential JSON shape for future admin-facing inspection UX.
- Keeps existing request-log provider/model icon behavior intact.

### Trade-offs

- Secret leaf types are normalized to masked strings, so original scalar type information (number/bool/string) is not retained.
- A new cache type increases conversion steps at the service boundary.

## Follow-up Work

- If admin endpoints later expose redacted credential previews, use `ResolvedProviderConnection.redacted_secrets` (or the same masking utility) as the canonical shape-preserving representation.
- Consider applying the same “shape-preserving mask” policy to any other runtime caches that currently hold provider secrets.

## Attribution

This ADR was prepared through collaborative human + AI implementation and documentation work.
