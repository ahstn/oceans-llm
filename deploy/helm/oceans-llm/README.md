# Oceans LLM Helm Chart

This chart deploys the Oceans LLM gateway and admin UI to Kubernetes.

The public entry point is always the gateway. The admin UI service stays
cluster-internal and is reached through the gateway at `/admin`.

## Install From GHCR

```bash
helm install oceans-llm oci://ghcr.io/ahstn/charts/oceans-llm \
  --version <version> \
  --values values.yaml
```

Use [Kubernetes and Helm](../../../docs/setup/kubernetes-and-helm.md) for the full chart contract.

Release chart packages set `appVersion` to the release tag, for example `v0.4.0`.
The default image tags are empty in `values.yaml`, so gateway and admin UI images
follow chart `appVersion` unless `gateway.image.tag` or `adminUi.image.tag`
explicitly override them.

## Required Runtime Secrets

The gateway config uses `env.*` references. Provide those values through one or
more of:

- `database.external.existingSecret` for `POSTGRES_URL`
- `secrets.existingSecret.name`
- `secrets.inline`
- `externalSecrets.enabled`

For production installs, provide at least:

- `POSTGRES_URL`
- `GATEWAY_IDENTITY_TOKEN_SECRET`
- provider credentials referenced by `gateway.config.providers`

## Database Modes

`database.mode: external` is the default. It expects a PostgreSQL URL from a
Kubernetes Secret.

`database.mode: cloudnativepg` renders a CloudNativePG `Cluster` resource. The
CloudNativePG operator and CRDs must already exist in the cluster. Configure
storage deliberately before using this mode in EKS or any persistent cluster.

## Startup Jobs

The chart renders a Helm hook migration Job by default. The default hook phase is
post-install and post-upgrade so chart-rendered ConfigMaps and Secrets exist
before the Job starts. Gateway pods run with startup mutations disabled:

- `GATEWAY_RUN_MIGRATIONS=false`
- `GATEWAY_BOOTSTRAP_ADMIN=false`
- `GATEWAY_SEED_CONFIG=false`

Bootstrap-admin and seed-config Jobs are opt-in through:

- `bootstrapAdminJob.enabled`
- `seedConfigJob.enabled`

Gateway pods wait for `gateway migrate --check` when migrations run in a
post-install or post-upgrade hook phase. Tune that wait with:

- `gateway.migrationWaiter.enabled`
- `gateway.migrationWaiter.intervalSeconds`
- `gateway.migrationWaiter.timeoutSeconds`

## Examples

Render the checked-in examples with:

```bash
mise run helm-template
```

Example values live in [examples](examples):

- external PostgreSQL
- inline secrets
- ExternalSecret
- ingress with TLS and HPA behavior
- CloudNativePG
- observability sidecar wiring

## Publishing

Release tags publish this chart to GHCR:

- chart reference: `oci://ghcr.io/ahstn/charts/oceans-llm`
- chart version: `X.Y.Z` from tag `vX.Y.Z`
- chart appVersion: `vX.Y.Z`

## Validation

```bash
mise run helm-check
```
