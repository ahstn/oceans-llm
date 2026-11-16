# ADR Safe Helm upgrades and rollback

- Date: 2026-11-15
- Status: Accepted

## Context

A Helm upgrade of `deploy/helm/oceans-llm` could drop work and could not undo itself safely:

- The gateway ran as PID 1 with no SIGTERM handler. Kubernetes killed it after the default 30-second grace period, which cut off long inference streams without a request log or usage record.
- Migrations ran in `post-install,post-upgrade` hooks. New pods started before the schema changed, and a failed migration left the release half applied.
- An older gateway refused any database that had a migration it did not know. After a migration, `helm rollback` could not work without a database restore.
- `/readyz` showed that a pod started. It did not show that the new version served requests well.

We compared blue/green, canary (Argo Rollouts, Flagger), and a rolling update with Helm's automatic rollback. A long-lived stream holds its connection to one pod, and the database is shared, so blue/green gains little over a well-drained rolling update. Argo Rollouts and Flagger need a controller and a metrics provider that this chart does not install.

## Decision

Keep the rolling update, and make each part of it safe:

1. **Graceful drain** (`crates/gateway/src/shutdown.rs`). The first SIGTERM starts a drain: the listener stops accepting connections, `/readyz` returns `503 draining`, and MCP listen streams end. In-flight requests continue for `server.shutdown_drain_seconds` (default 300). At the cutoff, a handler with no response yet returns `503 server_shutdown` without cancelling its accounting, and an open inference stream ends with a body error. Its request log records `503 server_shutdown` with the usage the provider already reported. The server owns its connections: after five seconds it aborts and joins those still open, including clients that stopped reading, so dropped bodies register their finalization before the final wait. Detached handlers, usage and request-log writes run in a `TaskTracker`, which the process then waits for, up to 10 seconds, before it exits. The 5 + 10 seconds fit the chart's 20-second post-drain allowance.
2. **Pod lifecycle** (chart). `preStop.sleep` (10 s) lets endpoints update before SIGTERM. `terminationGracePeriodSeconds` (330) must cover the sleep, the drain and 20 seconds, or rendering fails. The rollout uses `maxUnavailable: 0`, `maxSurge: 25%`, `minReadySeconds: 10` and a `/healthz` startup probe.
3. **Migrations before workloads**. The migration hook defaults to `pre-install,pre-upgrade`, with hook-scoped ServiceAccount, ConfigMap and Secret at earlier weights. CloudNativePG and External Secrets modes default to `post-install,pre-upgrade`, because their dependencies do not exist before the first install. The documented upgrade uses `--wait --rollback-on-failure`.
4. **Reader compatibility** (`crates/gateway-store/src/migration_registry.rs`, `src/migrate/compatibility.rs`). Each migration declares `ReaderCompatibility::Additive` or `RequiresReader(version)`. The runner stores the oldest readable version in `gateway_migration_compatibility`, in the same transaction as the history row. A gateway that finds newer migrations starts only if each one allows its version, and never applies its own pending migrations to such a database. Migrations V17 to V81 stay strict. A test fails if a new migration does not declare its compatibility, and additive migrations are linted for destructive SQL.
   The gateway checks compatibility at startup even when it does not run migrations.
5. **Traffic-based rollout guard** (`crates/gateway/src/http/traffic.rs`, `src/rollout_guard.rs`). The gateway counts responses per process, except health checks, and splits 5xx responses into gateway errors and upstream errors with a response extension set where error responses are built. A separate listener (`server.management_bind`) serves the counters at `/internal/traffic`. The opt-in `post-upgrade` Job runs `gateway rollout-guard`. It resolves the headless management Service each interval and adds the counter deltas per instance. After `minRequests` (50), it fails as soon as gateway errors reach 5% or upstream errors reach 25%. With less traffic, the window (300 s) passes.

## Why

- A drained rolling update protects long streams at the pod level, which is where they are pinned. It needs no new controller.
- Migrating before workloads means a failed migration changes nothing that serves traffic. Helm's rollback then restores the last working release.
- Recording reader compatibility in the database, written by the newer gateway, lets an older binary decide safely without knowing future migrations.
- Reading the gateway's own counters gives the guard real traffic without an API key, a metrics backend or RBAC. Splitting errors by origin keeps a provider outage from blocking an upgrade at the same rate as a gateway bug.

## Trade-offs

- A pod can take up to 330 seconds to stop, so node drains and rollouts take longer.
- New connections are refused during the drain. A pod still in the endpoints after the preStop sleep loses those connections.
- The guard runs only on `helm upgrade` with `--wait`, adds its window to every upgrade, and passes when traffic is low. It is off by default.
- The guard counts only responses that finished, by process. A pod that restarts loses its counts, and a stream that fails after its headers counts as a success.
- Rollback after a migration works only for releases that know this check and only across migrations that declare themselves additive. Until then, rollback still needs a database restore.
- Compose deployments still stop after Docker's 10-second default unless `stop_grace_period` is raised.

## Follow-ups

- A pre-upgrade backup Job, so that every upgrade has a restore point.
- Richer request metrics for an external analysis tool, such as Argo Rollouts or Flagger.
- An Argo Rollouts `workloadRef` option for users who want canary steps.
