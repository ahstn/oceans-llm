# Oceans LLM Helm Chart

This chart deploys the Oceans LLM gateway and admin UI to Kubernetes.

The public entry point is always the gateway. The admin UI service stays
cluster-internal and is reached through the gateway at `/admin`.

## Install From GHCR

```bash
helm install oceans-llm oci://ghcr.io/ahstn/charts/oceans-llm \
  --namespace <namespace> \
  --version <version> \
  --values values.yaml
```

Use [Kubernetes and Helm](https://oceans-llm.com/setup/kubernetes-and-helm/) for the full chart contract.

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
- `OCEANS_API_KEY_SECRET_ENCRYPTION_KEY` when declaring managed service-account keys in `gateway.config.service_accounts`

Set `gateway.clientConfigGatewayBaseUrl` when users will copy generated client
configuration snippets from `/admin/models`. Use the public gateway API base URL,
including `/v1`, for example `https://gateway.example.com/v1`.

By default, the chart rejects `literal.*` references in `gateway.config` because
the config is rendered into a ConfigMap. Use `env.*` references backed by
Kubernetes Secrets. Set `gateway.allowLiteralSecretsInConfig=true` only for an
intentional exception.

## Database Modes

`database.mode: external` is the default. It expects a PostgreSQL URL from a
Kubernetes Secret.

`database.mode: cloudnativepg` renders a CloudNativePG `Cluster` resource. The
CloudNativePG operator and CRDs must already exist in the cluster. Configure
storage deliberately before using this mode in EKS or any persistent cluster.

## Skill Storage

Skill storage is disabled by default. Set `gateway.config.skills.enabled=true` and configure `gateway.config.skills.storage` to enable it. Use the [skill configuration reference](https://oceans-llm.com/extensions/skills/) for the complete gateway contract. The gateway database stores skill metadata and versions; S3 stores the archive objects. Clients authenticate to Oceans and download through the gateway. They do not receive storage credentials.

The chart supports an existing S3 service or the optional upstream RustFS dependency:

| Deployment | Example | Storage responsibility |
| --- | --- | --- |
| AWS S3 or another existing S3 service | [skills-s3-values.yaml](examples/skills-s3-values.yaml) | Admins provision the private bucket and application identity. |
| RustFS distributed in the Oceans release | [skills-rustfs-values.yaml](examples/skills-rustfs-values.yaml) | The official RustFS chart provisions its workloads and PVCs. Admins supply the CSI driver and StorageClass. |
| RustFS standalone for development | [skills-rustfs-standalone-values.yaml](examples/skills-rustfs-standalone-values.yaml) | One data PVC; no RustFS node redundancy. |

Each example is a complete values overlay with a PostgreSQL Secret reference. Replace the example bucket, Secret names, capacity, and StorageClass before installation. Continue to supply the required gateway identity and provider secrets listed above.

### Credentials and Bucket Setup

Create the private `oceans-skills` bucket before using skill uploads. Give the gateway an application identity with only the required object access within the configured `skills/` prefix. Store `OCEANS_SKILLS_S3_ACCESS_KEY_ID` and `OCEANS_SKILLS_S3_SECRET_ACCESS_KEY` in the `oceans-skills-s3` Secret used by the examples. Optional temporary credentials use `gateway.config.skills.storage.session_token`, which must also refer to a Secret-backed value. With AWS workload identity, omit all three credential references and configure the gateway service account for the SDK credential chain.

Credential fields accept `env.*` or `file.*` references. For `file.*`, supply a Secret volume through `gateway.extraVolumes` and `gateway.extraVolumeMounts`. These volumes and mounts apply to the gateway, its migration waiter, and the migration, bootstrap-admin, and seed-config jobs. Create referenced Secrets before these workloads run, including pre-install hooks. Raw credentials are rejected. The existing `gateway.allowLiteralSecretsInConfig` exception applies to explicit `literal.*` references; keep it disabled for normal deployments.

RustFS uses a separate `oceans-skills-rustfs-root` Secret containing `RUSTFS_ACCESS_KEY` and `RUSTFS_SECRET_KEY`. This Secret belongs to RustFS and is not mounted in the gateway. Admins use it to create the bucket and the restricted application identity. The chart does not create buckets, generate application identities, or copy administrative credentials into the application Secret. `rustfs.localEndpointHost.autoInject=true` in the distributed example assumes the root Secret contains credentials only, with no runtime overrides.

The RustFS examples set `fullnameOverride: oceans-skills-rustfs` so the service name matches `gateway.config.skills.storage.endpoint`. Change both together if several releases share a namespace. They use the private HTTP endpoint with `allow_http: true` and path-style S3 addressing. Enable upstream TLS and use an HTTPS endpoint with `allow_http: false` when your network policy requires encrypted pod traffic. RustFS ingress and Gateway API exposure are disabled by the parent chart. External S3 does not require any RustFS resources.

### RustFS Dependency and CSI Storage

The chart pins the [official RustFS chart](https://charts.rustfs.com/) to `1.0.1`; it does not contain copied RustFS workloads. `rustfs.enabled` defaults to `false`. All remaining `rustfs` values pass to the upstream chart. Use the same release for the chart and image unless a tested upgrade requires an override.

Build a source checkout from its checked-in dependency lock:

```bash
mise run //helm:deps
mise run //helm:check
```

Maintainers use `mise run //helm:deps:update` only when updating the dependency and review both `Chart.yaml` and `Chart.lock`. CI builds the locked dependency before validation and includes it in the packaged Oceans chart.

Set `rustfs.storageclass.name` to an existing CSI-backed StorageClass. CSI supplies filesystem storage to RustFS pods; RustFS supplies the S3 API. The chart does not install a CSI driver or define a StorageClass. Admins must select the volume binding mode, reclaim policy, expansion support, and backup process. A topology-bound driver can require `WaitForFirstConsumer` so provisioning follows pod scheduling constraints. See [Kubernetes StorageClasses](https://kubernetes.io/docs/concepts/storage/storage-classes/).

The distributed example uses four pods with one 20 GiB data PVC and one 1 GiB log PVC per pod. Default upstream anti-affinity requires separate hosts; confirm that the cluster has enough nodes. The example requests are starting values, not capacity or availability guarantees. Set CPU, memory, capacity, and fault-domain rules for the workload. Its disruption budget permits one unavailable pod. RustFS storage capacity is lower than raw PVC capacity because erasure coding uses part of each volume.

### Retention, Upgrades, and Recovery

Back up both database metadata and object storage, and test their recovery together. The standalone chart keeps its generated PVCs on Helm removal. Distributed storage uses StatefulSet claim templates. Neither behavior protects data after an admin deletes the PVC or the storage backend applies its reclaim policy. Inspect the StorageClass policy before removal and use backups for recovery.

Keep an existing distributed pool's drive count and endpoints fixed. Changing `drivesPerNode` changes immutable StatefulSet claim templates. Use the upstream pool expansion procedure to add capacity; do not use ordinary Deployment scaling rules for object storage. Standalone storage cannot expand in place into a distributed cluster. Create the new cluster and migrate objects through S3. Verify pod replacement, data recovery, and the required failure tolerance before treating a deployment as supported. Helm rendering checks validate configuration only.

## Startup Jobs

The chart runs migrations in `pre-install,pre-upgrade` hooks by default, before
workloads change. Hook-scoped ServiceAccount, ConfigMap and inline Secret
resources run at weights -21, -20 and -19, before the migration Job at -10.
External database and referenced Secrets must already exist.

With `database.mode: cloudnativepg` or `externalSecrets.enabled`, the default
becomes `post-install,pre-upgrade`: the Cluster or generated Secret must exist
before the first migration, but upgrades migrate before replacing workloads.
Explicit `migrations.hook` overrides are honored; `pre-install` overrides are
rejected in these modes because their dependencies do not exist yet.

Argo CD maps `pre-upgrade` to PreSync and `post-install` to PostSync, and does not
distinguish installation from upgrade. The default CloudNativePG first sync
would therefore run a PreSync migration before its Cluster exists (the same
applies to External Secrets). For Argo CD, provision the database and generated
Secret in a separate, earlier application/sync, then override
`migrations.hook: pre-upgrade` so migration runs in PreSync against existing
dependencies on every Argo CD sync. This override is Argo-only: plain Helm would
skip migrations on installation with only `pre-upgrade`. Prefer
`database.mode: external` when a separate application owns the CloudNativePG
Cluster.

Simply setting `migrations.hook: post-install,post-upgrade` is not sufficient:
Argo CD normally waits for gateway health before PostSync, while the gateway's
migration waiter cannot become ready until migrations complete. Keeping these
dependencies in one first sync requires explicit external migration orchestration
or Argo CD health customization; the default Helm lifecycle is not an Argo CD
first-sync strategy.

Gateway pods run with startup mutations disabled:

- `GATEWAY_RUN_MIGRATIONS=false`
- `GATEWAY_BOOTSTRAP_ADMIN=false`
- `GATEWAY_SEED_CONFIG=false`

Bootstrap-admin and seed-config Jobs are opt-in through:

- `bootstrapAdminJob.enabled`
- `seedConfigJob.enabled`

Gateway pods validate the gateway configuration once, then wait for
`gateway migrate --check` when the effective migration hook includes
`post-install` or `post-upgrade`. Invalid configuration stops the init container
before migration polling starts. `gateway.migrationWaiter.enabled: auto` therefore
enables the waiter for CloudNativePG and External Secrets, but not the default
external-database mode. Tune the wait with:

- `gateway.migrationWaiter.enabled`
- `gateway.migrationWaiter.intervalSeconds`
- `gateway.migrationWaiter.timeoutSeconds` (default 600)

Hook Jobs default to `hookDeletePolicy: before-hook-creation` and
`ttlSecondsAfterFinished: 300`, which preserves completed Jobs briefly for
inspection and removes the previous hook before a new run. Add
`hook-succeeded` to a Job's `hookDeletePolicy` when successful Jobs should be
deleted immediately.

## Safe upgrades and rollback

The default RollingUpdate keeps `maxUnavailable: 0` and `maxSurge: 25%`.
`rollout.minReadySeconds: 10` requires stable readiness before replacing another
pod; `rollout.progressDeadlineSeconds: 900` exceeds the 600-second migration
waiter. The gateway startup probe checks `/healthz` every two seconds, allowing
30 failures before restart. Liveness and startup probes use a separate
30-second termination grace; readiness never sets probe-level termination grace.

On pod termination, `gateway.lifecycle.preStop.sleep.seconds: 10` gives endpoint
changes time to propagate before SIGTERM. Native lifecycle sleep requires
Kubernetes 1.30 or newer; chart rendering is validated against Kubernetes 1.35.
The gateway then drains in-flight requests for
`gateway.config.server.shutdown_drain_seconds` (default 300). The pod's
`gateway.terminationGracePeriodSeconds: 330` includes sleep, drain and 20 seconds
for final writes and shutdown. Rendering fails unless:

```text
terminationGracePeriodSeconds >= preStop.sleep.seconds + shutdown_drain_seconds + 20
```

Increase the grace period together with a longer drain or sleep. Configure
ingress/load-balancer timeouts for long-lived streams too; see the
[ingress example](examples/ingress-hpa-values.yaml).

For Helm 4, upgrade with automatic rollback on failure:

```bash
helm upgrade oceans-llm oci://ghcr.io/ahstn/charts/oceans-llm \
  --namespace <namespace> \
  --version <version> \
  --values values.yaml \
  --wait \
  --rollback-on-failure \
  --timeout 15m
```

Helm's `--timeout` also bounds hook Jobs. Allow enough time for migrations,
workload readiness and the guard's full `rolloutGuard.windowSeconds` observation
window; raise the timeout if those settings need longer.

Enable `rolloutGuard.enabled` to add a post-upgrade traffic check, after the
bootstrap and seed hooks. See [rollout-guard-values.yaml](examples/rollout-guard-values.yaml).
It resolves ready gateway pods through a private headless management Service and
polls their process-local traffic counters every 15 seconds for 300 seconds.
The chart injects `server.management_bind: "0.0.0.0:9090"` into the effective
gateway config; `rolloutGuard.managementPort` changes the port. A conflicting
explicit bind is rejected. The Service is not routed through ingress; restrict
pod-network access to management endpoints using your network policy.

The guard sums counter deltas across observed instances, separating gateway
5xx errors from upstream-provider 5xx errors (including upstream transport
failures and timeouts). HTTP 4xx responses are not errors. Once at least
`rolloutGuard.minRequests: 50` requests have been observed, it fails when gateway
errors reach `rolloutGuard.maxGatewayErrorPercent: 5` or upstream errors reach
`rolloutGuard.maxUpstreamErrorPercent: 25`. With less traffic, the window passes
without enough evidence to assess error rates; no responding target during the
entire window fails. Generate representative traffic if a meaningful rollout
decision is required. The Job has no database credentials or gateway config mount.

Database migrations are forward-only: automatic or manual rollback does not undo
schema changes. Rolling back after a migration requires that migration to be
reader-compatible with the older binary. Keep schema changes compatible with
both versions during rollout and rollback; an incompatible migration prevents
the older binary from using that database. `helm rollback` never runs migrations
or the rollout guard, and the guard does not run on first installation.

## Examples

Render the checked-in examples and validate them against Kubernetes schemas with:

```bash
mise run //helm:template
```

Example values live in [examples](examples):

- external PostgreSQL
- inline secrets
- ExternalSecret
- ingress with TLS and HPA behavior
- CloudNativePG
- observability sidecar wiring
- direct OpenTelemetry export to an existing Datadog Agent DaemonSet
- skill archives in external S3
- the official RustFS dependency with distributed or standalone CSI storage
- opt-in post-upgrade traffic guard with two gateway replicas

## Publishing

Release tags publish this chart to GHCR:

- chart reference: `oci://ghcr.io/ahstn/charts/oceans-llm`
- chart version: `X.Y.Z` from tag `vX.Y.Z`
- chart appVersion: `vX.Y.Z`

## Validation

```bash
mise run //helm:check
```
