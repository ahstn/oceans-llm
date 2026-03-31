# Operator Runbooks

`See also`: [Runtime Bootstrap and Access](../setup/runtime-bootstrap-and-access.md), [Deploy and Operations](../setup/deploy-and-operations.md), [Deploy Compose](../../deploy/README.md), [Configuration Reference](../configuration/configuration-reference.md), [Identity and Access](../access/identity-and-access.md), [Observability and Request Logs](observability-and-request-logs.md)

This page is action-oriented. It is not the place for broad topology or config reference detail.

## First Deploy

- copy `deploy/.env.example` to `deploy/.env`
- set image tags and secret values
- inspect the mounted config at [../deploy/config/gateway.yaml](../../deploy/config/gateway.yaml)
- start the stack:

```bash
docker compose -f deploy/compose.yaml up -d
```

- confirm the containers are healthy
- call `/healthz` and `/readyz`
- confirm whether admin bootstrap is enabled in the mounted config before assuming `/admin` is ready
- confirm the seeded gateway key works for `/v1/models`

If the deploy path is meant to support the admin UI on first boot, the mounted config needs a real bootstrap-admin plan or a pre-existing admin row.

## Upgrade Flow

- pick the target image tags
- review release notes and image caveats
- confirm database backup or recreate policy for the target environment
- update `deploy/.env`
- restart the stack with the new tags
- recheck `/readyz`
- recheck admin login or seeded API-key access
- spot-check one live `/v1/*` request

If the change touches admin APIs, also recheck the live admin-backed pages rather than only the public API.

## Failed Migration Recovery

Start with the least destructive path.

- inspect gateway logs
- run the explicit migrate command against the active config:

```bash
mise run gateway-migrate
```

- confirm the database URL points at the intended backend
- confirm the process did not start with migrations disabled

Pre-v1 migration flattening is still pending. That follow-up is tracked in [issue #44](https://github.com/ahstn/oceans-llm/issues/44).

If the environment is disposable pre-v1 state, recreation can be safer than manual repair. If the environment is not disposable, stop and inspect the migration error before retrying.

## Broken Admin Login

Work through these checks in order:

- confirm `/admin` is being served through the gateway, not only through the UI server on `:3001`
- confirm bootstrap admin is enabled in the active config if this is a fresh environment
- confirm the bootstrap admin command against the active config:

```bash
mise run gateway-bootstrap-admin
```

- confirm the expected first-login rule
  - local config does not force password rotation
  - production-shaped local config does force password rotation
- confirm the session is not simply expired or stale

If the environment relies on OIDC, also review [oidc-and-sso-status.md](../access/oidc-and-sso-status.md). The current OIDC flow is still development-style.

## Provider Auth Failure

Provider auth failures usually come from config shape or missing secrets.

- confirm the provider exists in the active config
- confirm the secret references resolve in the runtime environment
- confirm `openai_compat` providers have a supported `pricing_provider_id`
- confirm Vertex routes use `<publisher>/<model_id>` in `upstream_model`
- confirm the route is enabled and has positive weight
- confirm the model is not only visible in `/v1/models`, but actually viable for the requested operation

If the symptom is “model is visible but fails,” follow [request-lifecycle-and-failure-modes.md](../reference/request-lifecycle-and-failure-modes.md).

## Missing OTLP Collector

The checked-in deploy path does not ship a collector by default.

If OTLP export is configured but no collector is reachable:

- inspect gateway startup logs
- confirm `server.otel_endpoint` and `server.otel_metrics_endpoint`
- confirm the collector address is reachable from the gateway container
- decide whether the environment should:
  - wire a real collector, or
  - run without one and rely on logs plus request-log storage

The request-log admin APIs can still work without a collector. OTLP export and request-log persistence are related, but they are not the same dependency.

## Secret Rotation Checkpoints

When rotating secrets, check the dependent path instead of rotating blindly.

### Gateway API key

- update the config source
- restart or reseed as needed
- verify `/v1/models` with the new key
- verify the old key fails if revocation was intended

### Bootstrap admin password

- rotate through the admin UI or the normal auth flow
- confirm the new password works
- confirm the old password does not

### Provider token or service account

- update the runtime secret source
- restart or reload the affected service path
- run one live request through the affected provider
- confirm request logs show the expected provider key

## What This Page Does Not Own

- compose file syntax: [../deploy/README.md](../../deploy/README.md)
- startup and first-access rules: [runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md)
- topology and same-origin contract: [deploy-and-operations.md](../setup/deploy-and-operations.md)
- identity lifecycle rules: [identity-and-access.md](../access/identity-and-access.md)
- request-log payload policy: [observability-and-request-logs.md](observability-and-request-logs.md)
