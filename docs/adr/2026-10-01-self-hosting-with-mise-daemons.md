# ADR: Self-Hosting with mise Daemons, Manual Pull-Based Updates, and Tailscale Serve

- Date: 2026-10-01
- Status: Accepted
- Supersedes: decision 2 of [2026-03-17 migration atomicity and local Postgres ops hardening](2026-03-17-migration-atomicity-and-local-postgres-ops-hardening.md) (pitchfork-first local Postgres via `pitchfork.toml` and a helper script)

## Current state

- [../setup/self-hosting-mise-daemons.md](../setup/self-hosting-mise-daemons.md)

## Context

We want to run Oceans LLM on one Linux host (example: a homelab server named `hogwarts`), update it on demand, and reach it only over Tailscale. We do not want GitHub Actions or any other CI system to hold credentials for, or push to, that host.

Existing deploy paths do not fit:

- `deploy/compose.yaml` pulls GHCR images that GitHub Actions builds. The gateway image is `linux/amd64` only.
- The Helm chart needs Kubernetes.
- `mise run prod-stack` starts the stack in the foreground with `cargo run`, a dev config, and a literal admin password.

The repo also kept a separate Pitchfork setup for local Postgres: a root `pitchfork.toml` and a 150-line helper script. That setup served the same purpose as a mise `postgres` daemon.

## Decision

1. **One Postgres daemon for development and self-hosting.** The root `mise.toml` declares `[daemons.db]` with the `postgres` preset (`port = "auto"`, database `oceans_llm`). The `postgres:*` tasks in `crates/gateway-store/mise.toml` keep their names and call `mise daemons`. We deleted `pitchfork.toml` and `scripts/postgres-pitchfork.sh`. The local database now uses the preset's `postgres` user with `trust` authentication on loopback, instead of `oceans`/`oceans`. CI is unchanged: it uses its own Postgres service container and sets `POSTGRES_URL` and `TEST_POSTGRES_URL` explicitly.
2. **Self-hosting is a config environment.** `mise.selfhost.toml` adds the `gateway` and `admin-ui` daemons, the `deploy` task, and its helper tasks. It loads only in the `selfhost` environment. A git-ignored `.miserc.local.toml` selects it per checkout. It redeclares `db` (fixed port, `boot_start`) because a higher-precedence daemon declaration replaces the whole daemon.
3. **Build from source on the host.** `mise run deploy [ref]` fetches tags, checks out the latest `v*` tag (or a given ref), builds the UI and `cargo build --locked --release -p gateway`, and restarts the app.
4. **Supervise with `mise daemons`.** Mise generates the Pitchfork config. Daemons carry `boot_start = true` and `retry = 3`. `pitchfork boot enable` plus `loginctl enable-linger` starts the supervisor at boot.
5. **Manual updates.** An operator runs `deploy` over SSH or Tailscale SSH. No timer, webhook, or CI push exists. This is a deliberate choice for now.
6. **Tailscale Serve for ingress.** All listeners bind to `127.0.0.1`. `tailscale serve --bg --https=443 http://127.0.0.1:8080` publishes the gateway on the tailnet with an automatic certificate. The gateway proxies `/admin*` to the UI. Funnel stays off.
7. **Secrets in `mise.selfhost.local.toml`.** This is mise's own `mise.{env}.local.toml` mechanism. The file is git-ignored through `mise.*.local.toml`. The overlay declares each variable `required = true` and lists them under `redactions`. Encrypted options (`_.file` with sops, age values, fnox) remain available without changing the layout.
8. **Backup before restart.** `deploy` runs `pg_dump -Fc` into `.local/backups/` (mode 600) before it restarts anything.
9. **Separate gateway config.** `deploy/selfhost/gateway.yaml` binds to loopback and uses env-backed secrets. We did not reuse `deploy/config/gateway.yaml` (binds `0.0.0.0`, example users) or `gateway.prod.yaml` (literal admin password).
10. **UI listener host.** `crates/admin-ui/web/server.ts` now reads `HOST` (default `0.0.0.0`, unchanged). The daemon sets `HOST=127.0.0.1`. Bun otherwise listens on all interfaces, which exposes the UI port on the LAN.

## Why

- Building on the host removes the registry and CI from the trust path. Trust stays with the git remote and the tag you pick.
- `mise daemons` keeps daemons next to tasks and shares the tool environment. Pitchfork supplies restart, readiness checks, and boot start.
- The preset replaces custom `initdb`, readiness, and database-creation code. Developers and the self-hosted host use the same definition.
- Local-file overrides are a documented mise feature (`mise.{MISE_ENV}.local.toml`), validated against `required` variables, so no custom dotenv path is needed.
- Manual updates keep a human in the loop and need no inbound access.
- Loopback binds plus Serve give one audited ingress that Tailscale ACLs control.

## Alternatives Considered

- **Compose with GHCR images.** Rejected: depends on GitHub Actions output, and adds another supervisor.
- **Pitchfork `cron` poller on the host.** Deferred: unattended builds and restarts without review. Easy to add later as a daemon that runs `mise run deploy`.
- **Push from the laptop at the end of `mise run release`.** Rejected: adds a credential path from the laptop to the host.
- **Dedicated Tailscale Service name.** Rejected for now: needs extra tailnet policy for little gain on a single host.
- **Keep `pitchfork.toml` for dev and add mise daemons for production.** Rejected: two definitions of the same Postgres.
- **Plain dotenv file under `.local/`.** Rejected: works, but `mise.selfhost.local.toml` is the native mechanism and supports `required` and `redactions` checks.

## Consequences

Positive:

- One command updates the host. Rollback is another `deploy` with an older tag.
- No registry, CI secret, or inbound port is involved.
- Local Postgres has a single definition and no custom script.

Trade-offs:

- Developers need `pitchfork` installed (mise installs it as a tool) and `experimental = true`, which the repo already sets.
- The local dev database credentials changed. Existing `.local/postgres` data from the old script is unused and can be deleted.
- The first Rust build on the host is slow. Later builds reuse the cargo cache.
- The gateway is unavailable during restart. There is no blue/green.
- `mise daemons` is experimental upstream. Pin the `mise` version on the host.
- Backups live on the same disk.
- Rollback targets must contain `mise.selfhost.toml`. Migrations are not reversed, so a restore may be needed.

## Verification

On a macOS dev machine:

- `postgres:start`, `postgres:status`, `postgres:env`, and `postgres:reset` ran against the new `db` daemon from a linked worktree (automatic port offset).
- With `.miserc.local.toml` and a throwaway `mise.selfhost.local.toml`, `mise run selfhost-apply` built the stack and started all three daemons.
- The gateway, UI, and Postgres each listened on `127.0.0.1` only. `/readyz`, `/admin/login`, and `/v1/models` (with the seeded key) answered 200.
- A second run produced a `pg_dump` file and restarted the app daemons.
- `pitchfork supervisor run --boot` started the `boot_start` daemons.

Not verified: systemd registration, `loginctl enable-linger`, and `tailscale serve` on a Linux host.

## Follow-up Work

- Run the setup on the target host and confirm boot start and Tailscale Serve.
- Add off-host backup copies.
- Consider `git verify-tag` once release tags are signed.
- Consider a Pitchfork `cron` daemon for unattended updates if manual updates become a burden.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
