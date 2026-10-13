# ADR: Mise Monorepo Task Layout

- Date: 2026-10-13
- Status: Accepted

## Current state

- [Contributing](../../CONTRIBUTING.md)
- Root [`mise.toml`](../../mise.toml)

## Context

The root `mise.toml` held about 90 tasks. Most belonged to one project: `gateway-*`, `ui-*`, `helm-*`, `review-agent-action-*`, `e2e-*` and `harness-integration-*`. A few projects (`docs`, `tests/harness-integration`, `crates/gateway-store`) already had their own `mise.toml`, but they ran through `mise -C <dir>` or root wrappers. Many tasks also repeated the same env block, and inline scripts were wrapped in `set -euo pipefail` even though mise already runs inline scripts with `sh -o errexit`.

## Decision

### 1. Project tasks live next to their code

The root sets `monorepo_root = true` and lists each project under `[monorepo] config_roots`. Each project's `mise.toml` owns its tasks, and they run as `mise run //<path>:<task>`. `[monorepo.path_aliases]` gives short names, so `//ui:check` runs `//crates/admin-ui/web:check`.

The root keeps only tasks that are repo-wide or cross several projects:

- Rust workspace tasks;
- the aggregate `lint`, `build` and `test` tasks;
- the admin contract;
- the dev and prod stacks;
- release and `dist-*` tasks;
- guardrail release gates.

`[monorepo] lockfile = true` keeps a single root `mise.lock`.

### 2. Shared env comes from task templates

`[task_templates.gateway-local]` supplies the local development encryption key. `[task_templates.gateway-prod]` pins `gateway.prod.yaml` and marks `POSTGRES_URL` as required. Tasks use them through `extends`, and they use `${VAR:-default}` so an outer value still wins.

### 3. Variants become flags

Tasks that existed only to add a flag, such as `*-dry-run` and limit variants, are replaced by `usage` flags. Each flag also keeps its env var (`env="RETENTION"`), so existing env-based calls still work.

### 4. Scripts rely on mise's shell defaults

Inline scripts drop `set -e`, and `pipefail` stays only where a pipe exists. Multi-step tasks with no shared state use `run` arrays rather than one shell script.

## Reasons and tradeoffs

- Task names now show which project they belong to, and each project's tasks sit beside its code.
- Cross-project `depends` use full paths or aliases, so moving a directory means updating `config_roots`, the aliases, and every reference to it.
- Old flat names such as `ui-check` and `gateway-serve` are gone, with no compatibility shims. References in CI, docs, skills and the UI hint were updated in the same change. Historical plans and ADRs were not.
- `mise tasks ls` lists only root tasks; `mise tasks ls --all` lists everything.

## Follow up

- Add `mise tasks validate` to CI.
- In mise 2026.10.3, `mise daemons status <name>` errors when the daemon is not running. Retest after upgrading.
