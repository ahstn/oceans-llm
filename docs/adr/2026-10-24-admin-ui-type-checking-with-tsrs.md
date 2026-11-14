# ADR Admin UI type checking with tsrs

- Date: 2026-10-24
- Updated: 2026-11-14 (tsrs 0.25.0)
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

On 2026-11-14 the check was rerun on Linux x64 (Intel Core Ultra 7 265K, 20 threads) with `hyperfine`, 10 runs after 2 warm-ups:

| Checker | Wall time | Peak memory |
| --- | --- | --- |
| tsc 5.9.3 | 5.77 s | 931 MiB |
| tsc 6.0.3 | 5.86 s | 942 MiB |
| TypeScript 7.0.2 | 0.99 s | 975 MiB |
| TypeScript 7.1.0-beta | 0.82 s | 685 MiB |
| tsrs 0.18.0 | 0.23 s | 418 MiB |
| tsrs 0.25.0 | 0.33 s | 321 MiB |
| TypeScript 7.0.2, single-threaded | 1.72 s | 626 MiB |
| tsrs 0.18.0, single-threaded | 0.54 s | 188 MiB |
| tsrs 0.25.0, single-threaded | 0.55 s | 202 MiB |

tsrs 0.25.0 is about 40% slower than 0.18.0 in the default multi-threaded mode (its reported check time goes from 0.13 s to 0.23 s) but uses about a quarter less memory. It is still about 2.5 times faster than TypeScript 7.1.0-beta and 18 times faster than `tsc` 6.0. On a fixture with seven deliberate errors, tsrs 0.25.0, tsrs 0.18.0, TypeScript 7.0.2 and TypeScript 7.1.0-beta produced byte-identical output.

## Decision

Type-check the admin UI with tsrs. The `typecheck` script runs `tsrs --noEmit -p .`, and `tsrs` is a pinned dev dependency in `package.json` and `bun.lock`. Bun remains the only package manager and script runner for the admin UI.

## Implementation

- `crates/admin-ui/web/package.json` pins `tsrs@0.25.0-ts7.1.0-beta`. It installs one platform package from its optional dependencies: macOS arm64 and x64, Windows x64 and arm64, or Linux x64 and arm64 with glibc or musl. `bun.lock` records all eight.
- `mise run //ui:typecheck` and `mise run //ui:check` call `bun run typecheck`, so CI picks up tsrs without workflow changes. The tsrs launcher is a Node script; under `bun run` Bun stands in for `node` when Node is absent.
- `typescript` moves from 5.9.3 to 6.0.3, the last JavaScript release, and stays installed for `generate:admin-api` and editors. `openapi-typescript` 7.13.0 builds its output with the `ts.factory` API, which the TypeScript 7 package does not export, so generation crashes on 7. With 6.0.3 it produces output identical to the checked-in `admin-api.ts`, though its peer range is still `^5.x`. `tsconfck`, under `vite-tsconfig-paths`, has an optional `^5.0.0` peer and is unaffected.
- The Docker build does not type-check, so the image is unchanged.
- Zed uses tsrs as the language server for the admin UI. The root `.zed/settings.json` sets the binary of the `typescript-ls` server, from the "TypeScript Language Server" (tsgo) extension, to `crates/admin-ui/web/node_modules/.bin/tsrs --lsp --stdio`. `crates/admin-ui/web/.zed/settings.json` makes it the TypeScript and TSX server for that directory only, ahead of `vtsls` and `typescript-language-server`. Other TypeScript projects in the repository keep Zed's default server, because they still check with `tsc`.
- `CONTRIBUTING.md` and `crates/admin-ui/web/AGENTS.md` document the Bun and tsrs workflow.

## Reasons and tradeoffs

- Moving to TypeScript 7 alone gives most of the gain: about 6.5 times faster than `tsc` 5.9. tsrs is about 3.5 times faster again and uses less than half the memory, which also keeps the check cheap when several worktrees run it at once.
- tsrs targets TypeScript 7's diagnostics exactly, so switching to the official TypeScript 7 later is a script and dependency change only.
- tsrs is pre-1.0 and tracks TypeScript 7.1.0-beta. A tsrs bug could hide or invent an error that TypeScript 7 would not report.
- tsrs 0.18.0 had no binary for Intel macOS or Windows, so this change first shipped a TypeScript 7.0.2 fallback for those platforms. tsrs 0.20.0 added both, and the fallback was removed with the move to 0.25.0.
- Zed runs the pinned tsrs, so its errors match `bun run typecheck`. The setup needs the tsgo extension and `node` on Zed's `PATH`. Zed resolves the binary path from the open project's root, so the repository root must be the open project. The path is the Unix launcher, so it covers macOS and Linux, not Windows. Other editors run their bundled TypeScript or the workspace's `typescript` 6.0, so an error that changed between versions can appear in only one of them.
- Only the admin UI changes. `actions/review-agent` and `tests/harness-integration` keep `tsc`.

## Follow up

- Upgrade `typescript` to 7 so editors and the type check share TypeScript 7 semantics, once `openapi-typescript` works without the TypeScript 6 JavaScript API.
- If tsrs reports an error that TypeScript 7 does not, or misses one it does, switch the script to `tsc` from `typescript@7` and file the difference upstream.
- Revisit `actions/review-agent` and `tests/harness-integration` once tsrs has a stable release.
