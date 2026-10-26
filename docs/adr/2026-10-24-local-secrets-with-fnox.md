# ADR Local provider secrets with fnox

- Date: 2026-10-24
- Status: Accepted

## Context

Local provider keys lived in plaintext in the gitignored `mise.local.toml` `[env]`. mise loaded them into every shell in the checkout, so every process started there, AI agents included, inherited every key. `.worktreeinclude` copied the file into each new worktree, which left 19 plaintext copies across parallel worktrees.

The keys are stored in 1Password. The `op` CLI asks for authorization in each terminal tab and again after 10 minutes idle, and the limit can't be changed. Calling `op` directly from each shell therefore produced several prompts every half hour.

## Decision

Resolve local secrets with [fnox](https://fnox.jdx.dev), with 1Password as the source of truth.

- `fnox.toml` is committed and holds 1Password references only.
- mise's `[secrets.fnox]` gives a task only the keys listed in its `secrets`. The shell, `mise env`, hooks, and other tasks receive none.
- `mise run secrets-sync` caches the resolved values in the gitignored `fnox.local.toml`, encrypted with a personal age key. `.worktreeinclude` copies the cache into new worktrees.

## Implementation

- `fnox.toml` sets `root = true`, so a worktree under `.claude/worktrees/` doesn't load the main checkout's fnox files, and `env = "exec"`, so project keys stay out of the shell.
- The `op` and `sync-age` providers, along with keys shared across projects such as `OPENROUTER_API_KEY` and `EXA_API_KEY`, come from each developer's global `~/.config/fnox/config.toml`.
- Tasks that resolve provider `env.` tokens are granted them: `dev-stack` and the `//crates/gateway` `serve`, `seed-config`, `seed-local-demo`, and `reset-local-demo` tasks get `OPENAI_API_KEY` and `OPENROUTER_API_KEY`; `prod-stack`, `prod:serve`, and `prod:seed-config` get `OPENAI_API_KEY`.
- Existing checkouts must migrate. Move each provider key from `mise.local.toml` `[env]` into 1Password and delete it there, in the main checkout and in every worktree. mise refuses to run a task that is granted a key `[env]` also sets. Then run `mise tasks validate` and `mise run secrets-sync`.
- `.mcp.json` passes the ReUI license through `headersHelper` and `fnox get`, not by expanding an environment variable.
- The verify skill's `launch` runs `mise run dev-stack`. Without a terminal, mise runs fnox with `--non-interactive --no-daemon`, so agents depend on the `secrets-sync` cache.

## Why

- **Copies, not daemon:** fnox's in-memory daemon keys its cache on the set of config files, so every worktree misses it and asks 1Password again. The encrypted copy needs no 1Password call and works without a terminal, which is how agents run tasks.
- **Scoping:** per-task grants keep keys out of the environment that agents and unrelated tools inherit.
- **Single source:** 1Password stays the only store, and there is no separate secrets service to keep in sync.

## Trade-offs

- **Same-user access:** this prevents accidental exposure but is not a boundary against a deliberate same-user process. Anything running as the developer can read the age key or run `fnox get`.
- **Staleness:** the cache is copied once, when the worktree is created. After a key rotates, or after `fnox.toml` gains a key, rerun `mise run secrets-sync` in each live worktree. A stale local entry overrides the committed reference until then.
- **Requirements:** mise 2026.10.4 or later, fnox on `PATH`, and the experimental `[secrets.fnox]` feature.

## Follow-ups

- Revisit the copy model if fnox gains a cache shared across checkouts or a sync target outside the checkout.
- Consider `daemon_cache = false` and no sync for any future high-value credential.
