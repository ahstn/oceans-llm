# Kubernetes and Helm

`See also`: [Deploy and Operations](deploy-and-operations.md), [Runtime Bootstrap and Access](runtime-bootstrap-and-access.md), [Admin Runbooks](../operations/operator-runbooks.md), [Export Traces and Metrics](../operations/observability/export-traces-and-metrics.md), [Observability and Request Logs](../operations/observability-and-request-logs.md), [Configuration Reference](../configuration/configuration-reference.md), [Release Process](../contributing/reference/release-process.md), [Deploy](../../deploy/README.md)

This page owns the Kubernetes and Helm deployment contract for Oceans LLM.

## Chart Artifact

The supported chart is:

```bash
oci://ghcr.io/ahstn/charts/oceans-llm
```

Release tags publish chart version `X.Y.Z` from tag `vX.Y.Z`. The chart `appVersion` is set to the release tag, for example `v0.4.0`, and the default gateway and admin UI image tags follow that `appVersion` unless values override them.

Local chart checks live behind `mise`:

```bash
mise run //helm:check
```

That task updates chart dependencies, runs `helm lint`, and renders the default chart plus every checked-in example under [../../deploy/helm/oceans-llm/examples](../../deploy/helm/oceans-llm/examples). It validates each render with kubeconform against Kubernetes 1.35 schemas.

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

`gateway.config` is a structured values map. Helm renders it to `gateway.yaml` in a required ConfigMap and mounts it at `gateway.configMountPath`, which defaults to `/app/gateway.yaml`.

The chart intentionally has no raw YAML fallback value. Keep deploy-specific config in values files and let the chart render the gateway config.

Write `gateway.config.providers` and `gateway.config.models` as maps keyed by id. Helm deep-merges maps across `-f` values files but replaces lists, so map entries let each values file own a set of providers and models:

```bash
helm upgrade oceans oci://ghcr.io/ahstn/charts/oceans-llm \
  -f values.yaml -f models/openai.yaml -f models/multi-provider.yaml
```

An overlay removes an entry with `<id>: null`. The chart defaults both keys to `{}`; list-form values still work but Helm logs a "cannot overwrite table with non table" notice.

### Config Fragments From Your Own ConfigMaps

For a GitOps repository with one file per provider account, create the ConfigMaps yourself, for example with Kustomize `configMapGenerator`, and list them in `gateway.configDir.configMaps`:

```yaml
gateway:
  configDir:
    mountPath: /app/conf.d
    configMaps:
      - oceans-models-openai
      - oceans-models-shared
```

Each ConfigMap mounts at `<mountPath>/<name>/`, and the chart sets `GATEWAY_CONFIG_DIR` on the gateway pods and every hook Job. Key names only need to be unique within one ConfigMap. The gateway reads every YAML file below the directory and rejects an id defined twice. See [Config Files and Fragments](../configuration/configuration-reference.md#config-files-and-fragments) for the merge rules.

Limits to plan for:

- The ConfigMaps must exist before install, because the migration hook and the gateway init container load the config.
- The chart cannot checksum ConfigMaps it does not render, so a fragment change neither restarts pods nor reseeds. Applying a change takes two steps:
  - Reseed: pods do not seed, so enable `seedConfigJob.enabled` and run `helm upgrade`. Under Argo CD the seed Job runs as a `PostSync` hook whenever the release's Application syncs. That happens automatically only when the fragment ConfigMaps belong to the same Application; otherwise sync it manually.
  - Restart the gateway pods with `kubectl rollout restart deployment/<fullname>-gateway`, for example `deployment/oceans-llm-gateway` for a release named `oceans-llm`. Pods build provider clients at startup, so until they restart they keep the old endpoints and credentials and do not know about new providers. When Kustomize renders this chart with name hashes enabled, a changed ConfigMap gets a new name and the rollout happens on its own.
- Kustomize `configMapGenerator` appends a content hash to generated names by default, which the names in `gateway.configDir.configMaps` will not match. Set `options.disableNameSuffixHash: true` on each generator, unless Kustomize also renders this chart and rewrites the references.
- ConfigMap keys allow only letters, digits, `-`, `_`, and `.`, and each ConfigMap is limited to 1 MiB.

Set `gateway.clientConfigGatewayBaseUrl` to the public gateway API base URL that admins should copy into generated client snippets. This value is rendered as `GATEWAY_CLIENT_CONFIG_BASE_URL` on the gateway pod. Use the externally reachable OpenAI-compatible base URL, including `/v1`, for example:

```yaml
gateway:
  clientConfigGatewayBaseUrl: https://gateway.example.com/v1
```

Do not rely on the admin UI browser origin for this value. The admin UI may be served through a different ingress path or internal proxy than the API endpoint used by local harnesses.

## Secrets

The gateway config supports `env.*` and `literal.*` secret references. Kubernetes installs should use env-backed references for deploy-time secrets because Helm renders `gateway.config` into a ConfigMap.

By default, the chart rejects `literal.*` references in `gateway.config`. Set `gateway.allowLiteralSecretsInConfig=true` only when a deployment deliberately accepts the ConfigMap exposure.

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
- `OCEANS_API_KEY_SECRET_ENCRYPTION_KEY` when `gateway.config.service_accounts[*].keys` declares managed service-account keys
- `OCEANS_MCP_CREDENTIAL_ENCRYPTION_KEY` when users store upstream MCP credentials or `gateway.config.mcp.oauth.providers` is configured
- the MCP OAuth client id and secret referenced by `gateway.config.mcp.oauth.providers`

The MCP credential encryption key must decode to 32 bytes. Generate a separate random key and store it in the deployment secret manager. The chart loads the selected existing Secret or External Secret through `envFrom`, so `gateway.extraEnv` is not needed for these values.

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

Providers, models, teams, users, and service accounts in `gateway.config` reach the database only through the seed-config Job. With `seedConfigJob.enabled=false`, the default, a new install has no models until an admin creates them.

Migrations run as a Helm hook Job by default on install and upgrade. The default hook phase is post-install and post-upgrade so the Job can use the chart-rendered ConfigMap and Secret resources. Bootstrap-admin and seed-config are disabled by default and, when enabled, also run as Helm Jobs. They do not run inside replicated gateway pods.

Gateway pods include an automatic migration waiter when migrations run in a post-install or post-upgrade phase. The waiter runs `gateway migrate --check` until the schema is ready or `gateway.migrationWaiter.timeoutSeconds` is reached. That keeps gateway pods from serving ahead of the migration Job while still keeping the mutation itself in one Job.

Hook Jobs default to `helm.sh/hook-delete-policy: before-hook-creation` with `ttlSecondsAfterFinished: 300`. That keeps a successful Job around briefly for logs and debugging, while ensuring the previous hook resource is deleted before Helm creates the next one. Set `hookDeletePolicy` to include `hook-succeeded` if an environment should delete successful hook Jobs immediately.

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

Use `gateway.config.server.otel_endpoint`, `gateway.config.server.otel_metrics_endpoint`, and `gateway.config.server.otel_trace_sample_ratio` to configure gateway OTLP export. Use `observability.env` for environment variables required by sidecars or vendor agents. Trace sampling is parent-based and does not sample exported metrics. Examples cover OpenTelemetry and Datadog Agent-style wiring without making either one a bundled dependency.

Use [Export Traces and Metrics](../operations/observability/export-traces-and-metrics.md) for the complete setup and verification procedure.

## Example Values

Checked-in examples cover:

- external PostgreSQL
- providers and models from external fragment ConfigMaps
- inline Secret mode
- ExternalSecret mode
- ingress with TLS and HPA behavior
- CloudNativePG
- observability sidecar wiring
- direct OpenTelemetry export to an existing Datadog Agent DaemonSet

Render them locally with:

```bash
mise run //helm:template
```

## What This Page Does Not Own

- Gateway config field semantics: [Configuration Reference](../configuration/configuration-reference.md)
- General runtime shape: [Deploy and Operations](deploy-and-operations.md)
- First access behavior: [Runtime Bootstrap and Access](runtime-bootstrap-and-access.md)
- Action-oriented deployment and recovery steps: [Admin Runbooks](../operations/operator-runbooks.md)
- Release procedure: [Release Process](../contributing/reference/release-process.md)
