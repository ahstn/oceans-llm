# ADR Repository lint and dependency scanning

- Date: 2026-10-21
- Status: Accepted

## Context

Repository checks covered Rust and TypeScript sources, shell scripts, and workflow syntax. Nothing checked TOML, Markdown links, spelling, whitespace in other file types, or workflow security. Dependabot raised update pull requests, but no check failed when a change added a vulnerable dependency, and no policy covered Rust dependency licenses.

The first runs of the new tools found 14 broken documentation links and anchors, 23 workflow findings, and about 90 known advisories across the Cargo, Bun, and npm lockfiles.

## Decision

Add seven single-binary tools through mise, pinned in `mise.lock`:

- zizmor audits GitHub Actions workflows and Dependabot config.
- taplo formats and validates TOML.
- lychee checks Markdown links and heading anchors.
- editorconfig-checker enforces `.editorconfig` line endings, final newlines, and trailing whitespace.
- typos checks spelling.
- cargo-deny checks Rust advisories, licenses, and sources.
- osv-scanner checks every lockfile against OSV.

Dependency scanning gates new vulnerabilities, not the existing backlog. Pull requests fail when they introduce an advisory. The backlog is reported to code scanning weekly and is cleared by dependency upgrades.

## Implementation

- `mise run repo-lint` runs `toml-check`, `editorconfig-check`, `typos`, `links-check`, and `actions-audit`. `mise run lint` depends on it and on `rust-deny`. Each config file sits at the repository root, except `.github/zizmor.yml`.
- `.taplo.toml` associates the Cargo and mise JSON schemas with their files. taplo validates only files with an associated schema, and its default online catalog no longer parses.
- prek runs each check as its own hook. The typos hook checks staged files only. The existing `lint` hook only fires on Rust and TypeScript changes, so these hooks cover documentation and config commits.
- `.github/workflows/repo-lint.yml` runs on every pull request, including docs-only ones that `ci.yml` skips. It gives zizmor the read-only `GITHUB_TOKEN` for its online audits. `ci.yml` runs `rust-deny` next to Clippy.
- `.github/workflows/osv-scanner.yml` calls the upstream reusable workflows. On pull requests it scans the base and head commits and fails only on the difference. SARIF upload is off there, because fork pull requests get a read-only token. The weekly run uploads the full results without failing.
- `deny.toml` ignores the 19 RustSec advisories present at adoption. Each entry gives a reason. The license allowlist matches the current graph, and the workspace crates now declare `AGPL-3.0-only` to match `LICENSE`. utoipa reads that field, so the generated admin OpenAPI document's `info.license` now names it too.
- `.github/zizmor.yml` ignores the current findings in the cargo-dist generated `release.yml` by exact line and column, so a finding that regeneration moves fails again. `actions-audit` uses `--strict-collection`, so a workflow zizmor cannot parse fails instead of being skipped. It also disables the `self-repository` audit, because actionlint 1.7.12 rejects the `$/` syntax that audit recommends.
- `mise run deps-audit` runs osv-scanner over every tracked lockfile. It lists lockfiles with `git ls-files` because the scanner's recursive walk finds nothing in a linked git worktree.
- `mise run links-check-external` also fetches remote URLs. It stays out of CI because remote documentation sites change anchors and rate-limit requests.

- Two preview checks are installed but not part of `lint` or CI:
  - `mise run //store:migrations-lint` runs squawk over the Postgres migrations, using `.squawk.toml` for Postgres 18 and per-migration transactions. It reports 204 findings in the 58 existing migrations. Applied migrations are checksummed and cannot change, so these are informational.
  - `mise run //helm:validate` runs kubeconform over the chart defaults and every example. CRDs validate against datreeio/CRDs-catalog pinned to a commit. All rendered resources are valid.

## Reasons and tradeoffs

- Single-binary tools installed by mise keep local runs and CI identical without adding Node or Python dependencies. All of the checks in `repo-lint` finish in under a second.
- Gating only new advisories lets the checks ship now without a large upgrade change. The cost is that existing advisories stay until someone upgrades the affected crates and packages. The `deny.toml` baseline must shrink rather than grow.
- osv-scanner was chosen over Trivy, Grype, Snyk, Socket, and Semgrep Supply Chain. It is the only option that parses text `bun.lock`, `Cargo.lock`, and `package-lock.json` without an account. Its pull-request workflow compares base and head results. cargo-deny adds RustSec unmaintained notices and license and source policy, which osv-scanner does not cover.
- taplo was chosen over tombi. tombi re-sorts keys in 12 files and writes multi-line inline tables, which need TOML 1.1.
- Duplicate crate versions are allowed in `deny.toml`. They come from upstream crates such as the AWS SDK and libsql, and 69 warnings would bury real findings.

## Follow up

- Decide whether to gate squawk on new migrations only, and which rules to keep. The runner wraps each migration in a transaction, so `CREATE INDEX CONCURRENTLY` is unavailable and `require-concurrent-index-creation` cannot be satisfied without a runner change.
- Add `//helm:validate` to `helm-ci.yml` if it stays clean.

- Clear the advisory backlog, starting with zip 9 and rustls-webpki. Remove each `deny.toml` ignore as its advisory is fixed.
- Add container image and Helm misconfiguration scanning if the threat model needs it. Trivy covers both, but pin it through aqua rather than its GitHub Action after its March 2026 compromise.
