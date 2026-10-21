# ADR Repository lint and dependency scanning

- Date: 2026-10-21
- Status: Accepted

## Context

Repository checks covered Rust and TypeScript sources, shell scripts, and workflow syntax. Nothing checked TOML, Markdown links, spelling, whitespace in other file types, or workflow security. Dependabot raised update pull requests, but no check failed when a change added a vulnerable dependency, and no policy covered Rust dependency licenses.

The first runs of the new tools found 14 broken documentation links and anchors, 23 workflow findings, and about 90 known advisories across the Cargo, Bun, and npm lockfiles.

## Decision

Add ten single-binary tools through mise, pinned in `mise.lock`:

- zizmor audits GitHub Actions workflows and Dependabot config.
- tombi formats TOML and validates each file against the schema it declares.
- lychee checks Markdown links and heading anchors.
- editorconfig-checker enforces `.editorconfig` line endings, final newlines, and trailing whitespace.
- typos checks spelling.
- gitleaks scans new commits and staged changes for secrets.
- cargo-deny checks Rust advisories, licenses, and sources.
- osv-scanner checks every lockfile against OSV.
- squawk lints new Postgres migrations.
- kubeconform validates rendered Helm manifests.

Dependency scanning gates new vulnerabilities, not the existing backlog. Pull requests fail when they introduce an advisory. The backlog is reported to code scanning weekly and is cleared by dependency upgrades.

## Implementation

- `mise run repo-lint` runs `toml-check`, `editorconfig-check`, `typos`, `secrets-check`, `links-check`, and `actions-audit`. `mise run lint` depends on it and on `rust-deny`. Each config file sits at the repository root, except `.github/zizmor.yml`.
- Every TOML file starts with a `#:schema` directive, so editors and tombi validate it against that schema. `deny.toml`, `dist-workspace.toml`, `rust-analyzer.toml`, and `.squawk.toml` have no published schema; their first line links the configuration reference instead. `tombi.toml` keeps Cargo manifests on TOML 1.0, because the SchemaStore Cargo schema declares TOML 1.1 and tombi would otherwise rewrite dependency tables as multi-line inline tables. It also turns off schema-driven key and array reordering and preserves string quote styles.
- gitleaks scans every commit since the change base (`secrets-check`), including each commit on a pull request's branch, so a secret added and later removed in the same pull request still fails. `repo-lint.yml` checks out full history for this. The prek hook scans a copy of the staged files with `gitleaks dir` (`secrets-check-staged`), because `gitleaks git --pre-commit --staged` misses some secrets in 8.30.1 (gitleaks/gitleaks#2281). Neither reads the working tree, so gitignored local secrets such as `mise.local.toml` never match. `.gitleaks.toml` extends the default rules and allowlists exact fixture values only: two provider and model identifiers, placeholder `gwk_` keys, one throwaway RSA test key, and fake tokens in redaction tests. A real secret in the same file or field is still reported. With it, the full history (`secrets-check-history`) is clean. The tasks turn off `color.ui`, because coloured `git log -p` output makes gitleaks scan nothing.
- prek runs each check as its own hook. The typos hook checks staged files only. The existing `lint` hook only fires on Rust and TypeScript changes, so these hooks cover documentation and config commits.
- `.github/workflows/repo-lint.yml` runs on every pull request, including docs-only ones that `ci.yml` skips. It gives zizmor the read-only `GITHUB_TOKEN` for its online audits. `ci.yml` runs `rust-deny` next to Clippy.
- `.github/workflows/osv-scanner.yml` calls the upstream reusable workflows. On pull requests it scans the base and head commits and fails only on the difference. Lockfile changes pushed to `main` run the full scan straight away. SARIF upload is off there, because fork pull requests get a read-only token. The weekly run uploads the full results without failing.
- `deny.toml` ignores the 19 RustSec advisories present at adoption. Each entry gives a reason. The license allowlist matches the current graph, and the workspace crates now declare `AGPL-3.0-only` to match `LICENSE`. utoipa reads that field, so the generated admin OpenAPI document's `info.license` now names it too.
- `.github/zizmor.yml` ignores the current findings in the cargo-dist generated `release.yml` by exact line and column, so a finding that regeneration moves fails again. `actions-audit` uses `--strict-collection`, so a workflow zizmor cannot parse fails instead of being skipped. It also disables the `self-repository` audit, because actionlint 1.7.12 rejects the `$/` syntax that audit recommends.
- `mise run deps-audit` runs osv-scanner over every tracked lockfile. It lists lockfiles with `git ls-files` because the scanner's recursive walk finds nothing in a linked git worktree.
- `mise run links-check-external` also fetches remote URLs. It stays out of CI because remote documentation sites change anchors and rate-limit requests.

- squawk lints Postgres migrations. `repo-lint.yml` runs `mise run //store:migrations-lint-changed`, which checks only migrations added or edited since the base commit, because older migrations predate squawk. `.squawk.toml` sets Postgres 18, assumes one transaction per migration, and disables the concurrent-index rules that a transaction makes impossible. New migrations must set `SET LOCAL lock_timeout` and `statement_timeout`. `mise run //store:migrations-lint` reports all 174 findings in the 58 existing migrations.
- kubeconform validates the chart. `mise run //helm:template`, which `//helm:check` runs in `helm-ci.yml` and `release-product.yml`, renders the defaults and every example once and validates each render. Helm and kubeconform both target Kubernetes 1.35.9, the oldest minor still supported upstream. CRDs validate against datreeio/CRDs-catalog pinned to a commit.

## Reasons and tradeoffs

- Single-binary tools installed by mise keep local runs and CI identical without adding Node or Python dependencies. All of the checks in `repo-lint` finish in under a second.
- Gating only new advisories lets the checks ship now without a large upgrade change. The cost is that existing advisories stay until someone upgrades the affected crates and packages. The `deny.toml` baseline must shrink rather than grow.
- osv-scanner was chosen over Trivy, Grype, Snyk, Socket, and Semgrep Supply Chain. It is the only option that parses text `bun.lock`, `Cargo.lock`, and `package-lock.json` without an account. Its pull-request workflow compares base and head results. cargo-deny adds RustSec unmaintained notices and license and source policy, which osv-scanner does not cover.
- tombi replaced taplo. taplo's last release (0.10.0, May 2025) predates TOML 1.1.0 (December 2025), its default schema catalogue no longer parses, and `taplo lint` validates nothing unless each file is associated with a schema. tombi is actively maintained and reads `#:schema` directives, but its schema-driven reordering and the Cargo schema's TOML 1.1 setting need turning off to avoid churn.
- Duplicate crate versions are allowed in `deny.toml`. They come from upstream crates such as the AWS SDK and libsql, and 69 warnings would bury real findings.

## Follow up

- The migration runner records each applied migration by file name, not by content, so editing a released migration goes undetected and fresh installs diverge from upgraded ones. Unreleased migrations are edited routinely during a release cycle, so a guard should compare against the last release tag, or the registry should store a content digest.
- Supporting `CREATE INDEX CONCURRENTLY` would need a non-transactional migration mode in the runner. Re-enable the concurrent-index rules if that lands.

- Clear the advisory backlog, starting with zip 9 and rustls-webpki. Remove each `deny.toml` ignore as its advisory is fixed.
- Add container image and Helm misconfiguration scanning if the threat model needs it. Trivy covers both, but pin it through aqua rather than its GitHub Action after its March 2026 compromise.
