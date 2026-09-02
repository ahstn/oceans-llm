# Deploy Oceans LLM

Use this page to choose a deployment method, understand the required services, and verify an Oceans LLM deployment.

`See also`: [Configuration Reference](../configuration/configuration-reference.md), [Runtime Bootstrap and Access](runtime-bootstrap-and-access.md), [Kubernetes and Helm](kubernetes-and-helm.md), [Admin Runbooks](../operations/operator-runbooks.md), [Observability and Request Logs](../operations/observability-and-request-logs.md)

## Choose a deployment method

| Deployment | Intended use | Database | Runtime artifacts | First access |
| --- | --- | --- | --- | --- |
| Local development | Development and evaluation | LibSQL or SQLite | Local Rust and UI builds | Checked-in bootstrap admin |
| Production-shaped local | Pre-production testing | PostgreSQL | Local Rust and UI builds | Checked-in bootstrap admin with forced password rotation |
| Docker Compose | Single-host deployment | Included PostgreSQL container | Gateway and admin UI images from GHCR | Environment-backed bootstrap password and seeded API key |
| Kubernetes and Helm | Cluster deployment | External PostgreSQL or optional CloudNativePG | Gateway and admin UI images from GHCR | Opt-in bootstrap-admin or seed-config Jobs |

Use Docker Compose for a production-shaped single-host installation. Use the Helm chart when the environment requires Kubernetes scheduling, external secret management, ingress, or horizontal scaling.

The gateway container currently supports `linux/amd64`. The admin UI container supports `linux/amd64` and `linux/arm64`. A deployment that uses the published gateway image therefore needs `linux/amd64` capacity.

## Understand the runtime topology

Oceans LLM uses a same-origin control-plane topology:

1. The gateway serves the data-plane APIs under `/v1/*` and the admin APIs.
2. The admin UI server runs as a separate service.
3. The gateway proxies `/admin*` to the admin UI through `ADMIN_UI_UPSTREAM`.

Send all public HTTP traffic to the gateway. Do not expose the admin UI service directly through ingress or a public load balancer. Routing `/admin*` through the gateway preserves the expected authentication, static asset, and API behavior.

## Prepare a production deployment

Before deploying with Compose or Helm, prepare:

- `linux/amd64` capacity for the gateway image.
- A persistent PostgreSQL database. The Compose stack includes one; Helm uses external PostgreSQL by default and can optionally create a CloudNativePG `Cluster`.
- Provider credentials for every configured model route.
- A gateway identity-token secret.
- An API-key encryption key when the configuration declares managed service-account credentials.
- An MCP credential encryption key when users store upstream MCP credentials or MCP OAuth is configured.
- A first-access plan, either a bootstrap admin or an existing admin identity.
- DNS, TLS, and a reverse proxy or ingress for externally reachable deployments.
- A backup and restore policy for PostgreSQL before the first upgrade.

Keep deploy-time credentials in environment variables or the environment's secret manager. Do not put raw production secrets in gateway YAML, Helm values committed to source control, or Kubernetes ConfigMaps. See [Configuration Reference](../configuration/configuration-reference.md) for supported secret references.

## Run a local development stack

From the repository root, run:

```bash
mise run dev-stack
```

This starts the gateway and admin UI using `gateway.yaml` and a local LibSQL or SQLite database. Open `http://localhost:8080/admin` and sign in with the checked-in local bootstrap credentials documented in [Runtime Bootstrap and Access](runtime-bootstrap-and-access.md#local-development).

Use this runtime only for development and evaluation. Its checked-in credentials and demo data are not production defaults.

## Run a production-shaped local stack

Set `POSTGRES_URL` from a secret manager or an untracked environment file, then run:

```bash
# POSTGRES_URL=postgresql://USER:PASSWORD@HOST:5432/DATABASE, exported outside shell history
mise run prod-stack
```

This uses `gateway.prod.yaml`, PostgreSQL, and the local gateway and admin UI builds. The local bootstrap admin must rotate its password on first sign-in.

Use this runtime to test PostgreSQL, migrations, bootstrap behavior, and the same-origin UI before changing a deployed environment. It is not a substitute for testing the target Compose or Kubernetes configuration.

## Deploy with Docker Compose

The checked-in Compose stack runs the gateway, admin UI, and PostgreSQL. Its gateway configuration is mounted from `deploy/config/gateway.yaml`.

1. Copy the environment template:

   ```bash
   cp deploy/.env.example deploy/.env
   ```

2. Replace the example image versions, database credentials, gateway secrets, bootstrap password, public base URL, and provider credentials in `deploy/.env`.

3. Start the stack:

   ```bash
   docker compose -f deploy/compose.yaml up -d
   ```

4. Confirm that each container is running:

   ```bash
   docker compose -f deploy/compose.yaml ps
   ```

5. Complete the checks in [Verify the deployment](#verify-the-deployment).

The checked-in configuration seeds a `default` service account and its `GATEWAY_API_KEY`. It also creates a bootstrap admin at `admin@local` using `GATEWAY_BOOTSTRAP_ADMIN_PASSWORD`. Review [Runtime Bootstrap and Access](runtime-bootstrap-and-access.md#ghcr-compose-deploy) before changing the seed or bootstrap settings.

## Deploy with Kubernetes and Helm

The supported chart is published at `oci://ghcr.io/ahstn/charts/oceans-llm`. Use an explicit chart version rather than an unbounded version.

1. Prepare a namespace, PostgreSQL connection, runtime secrets, gateway configuration, ingress, and TLS.

2. Render and inspect the intended values:

   ```bash
   helm template oceans-llm oci://ghcr.io/ahstn/charts/oceans-llm \
     --version <version> \
     --values values.yaml
   ```

3. Confirm that ingress sends traffic only to the gateway service. Check the rendered database mode, secret references, migration Job, bootstrap settings, and seed settings.

4. Install the chart:

   ```bash
   helm install oceans-llm oci://ghcr.io/ahstn/charts/oceans-llm \
     --namespace <namespace> \
     --version <version> \
     --values values.yaml
   ```

5. Confirm that the migration Job succeeds and that the gateway and admin UI pods become ready.

6. Complete the checks in [Verify the deployment](#verify-the-deployment).

The migration Job is enabled by default. The bootstrap-admin and seed-config Jobs are disabled until explicitly enabled. Gateway pods do not run migrations, bootstrap an admin, or seed configuration on startup.

See [Kubernetes and Helm](kubernetes-and-helm.md) for database modes, required secrets, startup Jobs, ingress, scaling, scheduling, and example values.

## Connect observability

The gateway exports traces and metrics through OTLP. Configure these gateway fields as needed:

- `server.otel_endpoint`
- `server.otel_metrics_endpoint`
- `server.otel_trace_sample_ratio`
- `server.otel_export_interval_secs`

The checked-in Compose and Helm deployments do not install an OpenTelemetry Collector or vendor agent. The Helm chart provides environment, annotation, label, volume, and sidecar hooks for connecting an existing collector or agent.

Follow [Export Traces and Metrics](../operations/observability/export-traces-and-metrics.md) to configure and verify telemetry. Request-log persistence remains available when no OTLP collector is configured.

## Verify the deployment

Run these checks through the gateway service, reverse proxy, or ingress that clients will use.

1. Check liveness. A successful response confirms that the gateway process is running:

   ```bash
   curl --fail https://<oceans-host>/healthz
   ```

2. Check readiness. A successful response confirms that the gateway is ready to receive traffic:

   ```bash
   curl --fail https://<oceans-host>/readyz
   ```

3. Confirm data-plane authentication and model visibility:

   ```bash
   curl --fail \
     -H "Authorization: Bearer ${GATEWAY_API_KEY}" \
     https://<oceans-host>/v1/models
   ```

4. Open `https://<oceans-host>/admin` through the gateway and complete the intended first-access flow.

5. Send one low-cost request through a configured provider route.

6. Check gateway logs and the admin request logs for provider authentication, routing, or configuration errors.

A healthy process is not necessarily ready to serve model traffic. Treat `/readyz`, authenticated model discovery, admin access, and a provider-backed request as separate checks.

## Plan upgrades and rollback

Before an upgrade:

1. Select explicit image or chart versions.
2. Review release notes and configuration changes.
3. Back up PostgreSQL and verify the restore procedure.
4. Render the target Compose or Helm configuration where applicable.
5. Decide how to recover if the application changes succeed but a database migration cannot be reversed.

After an upgrade, repeat the readiness, admin login, model discovery, and provider request checks.

Application rollback does not roll back the database. Confirm schema compatibility before reverting images or using `helm rollback`. Follow [Admin Runbooks](../operations/operator-runbooks.md#upgrade-flow) for Compose and Helm upgrade steps, [Helm Rollback](../operations/operator-runbooks.md#helm-rollback) for Kubernetes rollback, and [Failed Migration Recovery](../operations/operator-runbooks.md#failed-migration-recovery) when startup is blocked by a migration.

## Next steps

- Configure first access and understand seeded state: [Runtime Bootstrap and Access](runtime-bootstrap-and-access.md)
- Configure models, providers, secrets, and runtime behavior: [Configuration Reference](../configuration/configuration-reference.md)
- Configure Kubernetes-specific behavior: [Kubernetes and Helm](kubernetes-and-helm.md)
- Operate upgrades and recover failures: [Admin Runbooks](../operations/operator-runbooks.md)
- Configure telemetry and request logs: [Observability and Request Logs](../operations/observability-and-request-logs.md)
