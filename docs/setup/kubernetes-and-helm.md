# Kubernetes and Helm

`See also`: [Deploy and Operations](deploy-and-operations.md), [Runtime Bootstrap and Access](runtime-bootstrap-and-access.md), [Admin Runbooks](../operations/operator-runbooks.md), [Observability and Request Logs](../operations/observability-and-request-logs.md), [Configuration Reference](../configuration/configuration-reference.md), [Release Process](../reference/release-process.md), [Deploy](../../deploy/README.md)

This page owns the Kubernetes and Helm deployment contract for Oceans LLM.

## Chart Artifact

The supported chart is:

```bash
oci://ghcr.io/ahstn/charts/oceans-llm
```

Release tags publish chart version `X.Y.Z` from tag `vX.Y.Z`. The chart `appVersion` is set to the release tag, for example `v0.4.0`, and the default gateway and admin UI image tags follow that `appVersion` unless values override them.

Local chart checks live behind `mise`:

```bash
mise run helm-check
```

That task updates chart dependencies, runs `helm lint`, and renders the default chart plus every checked-in example under [../../deploy/helm/oceans-llm/examples](../../deploy/helm/oceans-llm/examples).

## Runtime Topology

The chart deploys:

- gateway `Deployment` and `Service`
- admin UI `Deployment` and `Service`
- gateway config `ConfigMap`
- optional inline `Secret`
- migration `Job`
- optional bootstrap-admin and seed-config `Job`s
- optional `Ingress`, `HorizontalPodAutoscaler`, `PodDisruptionBudget`, `ExternalSecret`, and CloudNativePG `Cluster`

All public HTTP traffic enters through the gateway service. The admin UI service is cluster-internal and is only reached through the gateway at `/admin*`. The chart does not render direct admin UI ingress.

## Gateway Config

`gateway.config` is a structured values map. Helm renders it to `gateway.yaml` in a required ConfigMap and mounts it at `/app/gateway.yaml`.

The chart intentionally has no raw YAML fallback value. Keep deploy-specific config in values files and let the chart render the gateway config.

## Secrets

The gateway config supports `env.*` and `literal.*` secret references. Kubernetes installs should use env-backed references for deploy-time secrets.

Supported secret sources:

- `database.external.existingSecret` for `POSTGRES_URL`
- `secrets.existingSecret.name` for an existing app secret
- `secrets.inline` for chart-rendered secrets
- `externalSecrets.enabled` for an `ExternalSecret` that points at an existing `SecretStore` or `ClusterSecretStore`

For production-like installs, provide:

- `POSTGRES_URL` or CloudNativePG-generated connection secrets
- `GATEWAY_IDENTITY_TOKEN_SECRET`
- provider credentials referenced by `gateway.config.providers`
- any bootstrap-admin password used by an opt-in bootstrap Job

The chart does not install External Secrets Operator and does not create a store.

## Database Modes

`database.mode: external` is the default. It expects a PostgreSQL URL from a Kubernetes Secret or one of the shared secret sources.

`database.mode: cloudnativepg` renders a CloudNativePG `Cluster` custom resource. The CloudNativePG controller and CRDs must already exist. Configure persistence explicitly through `database.cloudnativepg.storage` and, when needed, `database.cloudnativepg.walStorage`.

The chart does not install CloudNativePG and does not install PostgreSQL outside the optional `Cluster` resource.

## Startup Jobs

Gateway pods run with startup mutation switches disabled:

- `GATEWAY_RUN_MIGRATIONS=false`
- `GATEWAY_BOOTSTRAP_ADMIN=false`
- `GATEWAY_SEED_CONFIG=false`

Migrations run as a Helm hook Job by default on install and upgrade. The default hook phase is post-install and post-upgrade so the Job can use the chart-rendered ConfigMap and Secret resources. Bootstrap-admin and seed-config are disabled by default and, when enabled, also run as Helm Jobs. They do not run inside replicated gateway pods.

Gateway pods include an automatic migration waiter when migrations run in a post-install or post-upgrade phase. The waiter runs `gateway migrate --check` until the schema is ready or `gateway.migrationWaiter.timeoutSeconds` is reached. That keeps gateway pods from serving ahead of the migration Job while still keeping the mutation itself in one Job.

CloudNativePG and ExternalSecret installs force migration hooks to post-install and post-upgrade. CloudNativePG needs the database custom resource created first. ExternalSecret installs need the ExternalSecret resource created before the target Kubernetes Secret can be materialized by External Secrets Operator.

## Traffic, Scaling, and Scheduling

`ingress.*` renders an Ingress to the gateway service only. Use your existing ingress controller, DNS, TLS, and certificate automation outside this chart.

`autoscaling.*` renders an `autoscaling/v2` `HorizontalPodAutoscaler` for the gateway. Metrics and behavior are pass-through Kubernetes HPA fields.

The chart renders a gateway `PodDisruptionBudget` automatically only when the gateway is configured for HA, either by `gateway.replicaCount > 1` or `autoscaling.minReplicas > 1`. Set `podDisruptionBudget.enabled` to `true` or `false` to override the automatic behavior.

Use `scheduling.*` for shared pod placement controls:

- resources
- node selectors
- affinity
- tolerations
- topology spread constraints
- priority class

These fields are intentionally generic so EKS, Karpenter, and non-EKS clusters can apply their own node and disruption policies without chart-specific branches.

The chart intentionally does not render Karpenter `NodePool`, `EC2NodeClass`, or disruption policy resources. Platform maintainers should manage those outside the chart, then connect workloads through labels, taints, tolerations, topology spread constraints, resource requests, and priority classes.

For Karpenter-backed clusters, keep HPA and node provisioning behavior aligned:

- set realistic gateway CPU and memory requests so HPA metrics and Karpenter bin packing use the same capacity signal
- use topology spread constraints when gateway HA should cross zones or capacity types
- avoid selectors that only match a narrow node pool unless that pool has enough capacity for surge and disruption
- confirm `podDisruptionBudget` settings do not conflict with voluntary node disruption budgets

## Observability

The chart does not install an OpenTelemetry Collector or vendor agent. It exposes generic hooks:

- `observability.env`
- `observability.podLabels`
- `observability.podAnnotations`
- `observability.volumes`
- `observability.volumeMounts`
- `observability.sidecars`

Use `gateway.config.server.otel_endpoint`, `gateway.config.server.otel_metrics_endpoint`, and `observability.env` to point the gateway at an existing collector, DaemonSet, sidecar, or vendor endpoint. Examples cover OpenTelemetry and Datadog Agent style wiring without making either one a bundled dependency.

## Example Values

Checked-in examples cover:

- external PostgreSQL
- inline Secret mode
- ExternalSecret mode
- ingress with TLS and HPA behavior
- CloudNativePG
- observability sidecar wiring

Render them locally with:

```bash
mise run helm-template
```

## What This Page Does Not Own

- Gateway config field semantics: [Configuration Reference](../configuration/configuration-reference.md)
- General runtime shape: [Deploy and Operations](deploy-and-operations.md)
- First access behavior: [Runtime Bootstrap and Access](runtime-bootstrap-and-access.md)
- Action-oriented deployment and recovery steps: [Admin Runbooks](../operations/operator-runbooks.md)
- Release procedure: [Release Process](../reference/release-process.md)
