# Admin Runbooks

`See also`: [Runtime Bootstrap and Access](../setup/runtime-bootstrap-and-access.md), [Deploy and Operations](../setup/deploy-and-operations.md), [Deploy](../../deploy/README.md), [Configuration Reference](../configuration/configuration-reference.md), [Identity and Access](../access/identity-and-access.md), [Observability and Request Logs](observability-and-request-logs.md)

This page is action-oriented. It is not the place for broad topology or config reference detail.

## First Deploy

### Compose

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

### Helm

- create the namespace and runtime secrets outside the chart
- render the intended values:

```bash
helm template oceans-llm deploy/helm/oceans-llm --values values.yaml
```

- confirm ingress routes only target the gateway service
- confirm `gateway.config.database.url` matches the selected database mode
- install the chart:

```bash
helm install oceans-llm oci://ghcr.io/ahstn/charts/oceans-llm \
  --version <version> \
  --values values.yaml
```

- confirm the migration Job completed
- confirm gateway and admin UI pods are ready
- call `/healthz` and `/readyz` through the gateway service or ingress
- confirm bootstrap-admin and seed-config Jobs were enabled only when intended
- inspect completed hook Job logs before TTL cleanup if migration or bootstrap behavior needs review

## Upgrade Flow

### Compose

- pick the target image tags
- review release notes and image caveats
- confirm database backup or recreate policy for the target environment
- update `deploy/.env`
- restart the stack with the new tags
- recheck `/readyz`
- recheck admin login or seeded API-key access
- spot-check one live `/v1/*` request

If the change touches admin APIs, also recheck the live admin-backed pages rather than only the public API.

### Helm

- pick the target chart version
- review release notes, chart values changes, and image caveats
- confirm database backup or recreate policy for the target environment
- render the upgrade:

```bash
helm template oceans-llm oci://ghcr.io/ahstn/charts/oceans-llm \
  --version <version> \
  --values values.yaml
```

- apply the upgrade:

```bash
helm upgrade oceans-llm oci://ghcr.io/ahstn/charts/oceans-llm \
  --version <version> \
  --values values.yaml
```

- confirm the migration hook Job completed
- recheck gateway rollout, `/readyz`, admin login, and one live `/v1/*` request
- inspect completed hook Job logs before TTL cleanup if the upgrade changed database or seed behavior

If the upgrade fails after chart rendering but before pods are healthy, inspect hook Jobs first, then the gateway deployment events.

## Helm Rollback

- inspect revisions:

```bash
helm history oceans-llm
```

- confirm the target revision and database compatibility
- roll back:

```bash
helm rollback oceans-llm <revision>
```

- confirm the gateway deployment becomes ready
- recheck `/readyz`, admin login, and one live `/v1/*` request

Do not treat Helm rollback as a database rollback. If a migration already changed the database, review the migration notes before rolling application code back.

## Helm Scheduling and HA Checks

For HA gateway installs:

- confirm `gateway.replicaCount > 1` or `autoscaling.minReplicas > 1`
- confirm the rendered `PodDisruptionBudget` matches the intended disruption budget
- confirm `scheduling.topologySpreadConstraints` and affinity rules do not make pods unschedulable
- if using Karpenter or another dynamic node provisioner, confirm node selectors, tolerations, and priority class match available node pools
- confirm HPA metrics are available before relying on autoscaling behavior

## Failed Migration Recovery

Start with the least destructive path.

- inspect gateway logs
- run the explicit migrate command against the active config:

```bash
mise run gateway-migrate
```

- confirm the database URL points at the intended backend
- confirm the process did not start with migrations disabled

If the migration error says `database reset required`, the running database carries pre-baseline history that this release no longer accepts. Recreate the libsql/Postgres database, then rerun migrations and seeding/bootstrap steps against the fresh `V17` baseline.

If the error is not a reset-required failure, stop and inspect it before retrying. Do not assume manual repair is safer than recreation.

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

For Helm installs, wire collector access through `gateway.config.server.otel_endpoint`, `gateway.config.server.otel_metrics_endpoint`, and `observability.*` values. The chart does not install a collector.

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
- Kubernetes chart contract: [kubernetes-and-helm.md](../setup/kubernetes-and-helm.md)
- startup and first-access rules: [runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md)
- topology and same-origin contract: [deploy-and-operations.md](../setup/deploy-and-operations.md)
- identity lifecycle rules: [identity-and-access.md](../access/identity-and-access.md)
- request-log payload policy: [observability-and-request-logs.md](observability-and-request-logs.md)
