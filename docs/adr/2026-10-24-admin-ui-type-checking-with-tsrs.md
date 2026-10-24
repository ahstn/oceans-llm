# ADR Admin UI type checking with tsrs

- Date: 2026-10-24
- Status: Accepted

## Context

The admin UI uses Bun to install packages and run scripts, but it type-checked with `tsc` from `typescript@5.9.3`. A full check of `crates/admin-ui/web` took about 5.3 seconds and 920 MiB, and it runs in `mise run //ui:check` locally and in `ui-ci.yml`.

TypeScript 7 (`typescript@7.0.2`), Microsoft's Go port, is now the stable release. tsrs is a function-by-function Rust port of TypeScript 7, developed in the Oxc project and announced on 2026-10-20. It ships prebuilt binaries through npm.

Measured on an Apple M4 Pro with `--noEmit -p .`, 10 runs after 2 warm-ups:

| Checker | Wall time | Peak memory |
| --- | --- | --- |
| tsc 5.9.3 | 5.35 s | 923 MiB |
| TypeScript 7.0.2 | 0.82 s | 982 MiB |
| tsrs 0.18.0 | 0.24 s | 416 MiB |
| TypeScript 7.0.2, single-threaded | 1.50 s | 613 MiB |
| tsrs 0.18.0, single-threaded | 0.46 s | 188 MiB |

All three report no errors on the admin UI. On a file with seven deliberate errors, tsrs and TypeScript 7 produced byte-identical output; `tsc` 5.9 differed only in property order inside one message.

## Decision

Type-check the admin UI with tsrs. The `typecheck` script runs `tsrs --noEmit -p .`, and `tsrs` is a pinned dev dependency in `package.json` and `bun.lock`. Bun remains the only package manager and script runner for the admin UI.

## Implementation

- `crates/admin-ui/web/package.json` pins `tsrs@0.18.0-ts7.1.0-dev.20260929`. It installs one platform package from its optional dependencies: macOS arm64, or Linux x64 and arm64 with glibc or musl. `bun.lock` records all five.
- `mise run //ui:typecheck` and `mise run //ui:check` call `bun run typecheck`, so CI picks up tsrs without workflow changes. The tsrs launcher is a Node script; under `bun run` Bun stands in for `node` when Node is absent.
- `typescript@5.9.3` stays installed. `openapi-typescript` declares a `typescript@^5.x` peer dependency for `generate:admin-api`, and editors can use it as the workspace TypeScript version.
- The Docker build does not type-check, so the image is unchanged.
- `CONTRIBUTING.md` and `crates/admin-ui/web/AGENTS.md` document the Bun and tsrs workflow.

## Reasons and tradeoffs

- Moving to TypeScript 7 alone gives most of the gain: about 6.5 times faster than `tsc` 5.9. tsrs is about 3.5 times faster again and uses less than half the memory, which also keeps the check cheap when several worktrees run it at once.
- tsrs targets TypeScript 7's diagnostics exactly, so switching to the official TypeScript 7 later is a script and dependency change only.
- tsrs is pre-1.0 and tracks a TypeScript 7.1 dev snapshot. A tsrs bug could hide or invent an error that TypeScript 7 would not report.
- tsrs has no binary for Intel macOS or Windows, so `typecheck` fails to start there. `typecheck:ts7` (`mise run //ui:typecheck:ts7`) runs TypeScript 7 through `bunx --package typescript@7.0.2` on those platforms. It matched tsrs on the tested fixture, but tsrs tracks a 7.1 snapshot, so diagnostics can differ between the two. It adds nothing to `bun.lock`, but it downloads TypeScript 7 on first use. `mise run //ui:check` runs `typecheck:any` (`scripts/typecheck.mjs`), which picks `typecheck` on darwin-arm64, linux-x64 and linux-arm64 and `typecheck:ts7` elsewhere.
- Editors run their bundled TypeScript or the workspace's `typescript` 5.9, while CI runs TypeScript 7.1 semantics through tsrs. An error that changed between versions can appear in only one of them.
- Only the admin UI changes. `actions/review-agent` and `tests/harness-integration` keep `tsc`.

## Follow up

- Upgrade `typescript` to 7 so editors and the type check share TypeScript 7 semantics, once `openapi-typescript` accepts it as a peer dependency.
- If tsrs reports an error that TypeScript 7 does not, or misses one it does, switch the script to `tsc` from `typescript@7` and file the difference upstream.
- Revisit `actions/review-agent` and `tests/harness-integration` once tsrs has a stable release.
