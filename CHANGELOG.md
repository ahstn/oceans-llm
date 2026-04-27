# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [0.6.0] - 2026-04-24
### :rocket: New features
- *(docs)* Publish docs site with vitepress by @ahstn
- *(docs)* Publish docs site with vitepress by @ahstn in [#75](https://github.com/ahstn/oceans-llm/pull/75)
- *(gateway)* Add declarative config seeding for teams and users by @ahstn
- *(gateway)* Add declarative config seeding for teams and users by @ahstn in [#79](https://github.com/ahstn/oceans-llm/pull/79)
- *(admin-ui)* Adopt shadcn sidebar preset layout by @ahstn
- *(gateway)* Seed richer local demo data by @ahstn
- *(admin-ui)* Add provider and model brand icons by @ahstn
- *(admin)* Improve provider branding and lookup efficiency by @ahstn
- *(admin-ui)* Add provider and model brand icons by @ahstn in [#81](https://github.com/ahstn/oceans-llm/pull/81)
- *(gateway)* Seed richer local demo data by @ahstn
- *(admin-ui)* Polish preset layout and harden admin models by @ahstn in [#83](https://github.com/ahstn/oceans-llm/pull/83)
- *(gateway)* Seed richer local demo data by @ahstn in [#82](https://github.com/ahstn/oceans-llm/pull/82)
- *(admin-ui)* Improve models page table scrolling by @ahstn
- *(admin)* Add observability usage leaderboard by @ahstn in [#85](https://github.com/ahstn/oceans-llm/pull/85)
- *(models)* Updating models api by @ahstn
- *(admin-ui)* Improve models page table scrolling by @ahstn in [#86](https://github.com/ahstn/oceans-llm/pull/86)
- *(gateway)* Add provider compatibility profiles by @ahstn
- *(gateway)* Add provider compatibility profiles by @ahstn in [#94](https://github.com/ahstn/oceans-llm/pull/94)
- *(admin)* Add current-session logout by @ahstn
- *(admin)* Add current-session logout by @ahstn in [#104](https://github.com/ahstn/oceans-llm/pull/104)
- *(gateway)* Add OpenAI Responses API support by @ahstn
- *(gateway)* Add OpenAI Responses API support by @ahstn in [#95](https://github.com/ahstn/oceans-llm/pull/95)
- *(gateway)* Harden request log payload policy by @ahstn in [#117](https://github.com/ahstn/oceans-llm/pull/117)

### :bug: Bug fixes
- *(api-keys)* Address rebase fallout and review findings by @ahstn
- *(gateway)* Normalize declarative identity config values by @ahstn
- *(gateway-store)* Guard seeded identity auth mutations by @ahstn
- *(admin)* Paginate models and redact provider cache by @ahstn
- *(gateway)* Keep local demo bootstrap-safe by @ahstn
- *(admin-ui)* Restore upstream model column layout by @ahstn
- *(admin-ui)* Restore upstream model column layout by @ahstn in [#87](https://github.com/ahstn/oceans-llm/pull/87)
- *(ui)* Fixing overscroll on main body content by @ahstn

### Build
- Post release tasks - v0.5.0 by @ahstn

### Changed
- Implement live admin API key management by @ahstn
- *(api-keys)* Harden admin lifecycle architecture by @ahstn
- *(api-keys)* Harden admin lifecycle architecture by @ahstn in [#73](https://github.com/ahstn/oceans-llm/pull/73)
- *(gateway-store)* Rebaseline pre-v1 migrations by @ahstn
- Fix declarative seed validation ordering by @ahstn
- *(main)* Resolve conflicts and harden migration reset detection by @ahstn
- *(gateway-store)* Rebaseline pre-v1 migrations by @ahstn in [#77](https://github.com/ahstn/oceans-llm/pull/77)
- Simplify local runtime setup with mise by @ahstn
- Merge remote-tracking branch 'origin/codex/seed-local-demo-data' into codex/ui-preset-polish-sync by @ahstn
- Polish API key management flows by @ahstn
- Add observability usage leaderboard by @ahstn
- Normalize generated admin API typings by @ahstn
- Merge branch 'main' into codex/models-page-scroll-refresh by @ahstn
- Fix admin UI localhost SSR auth flow by @ahstn
- Harden request log payload policy by @ahstn
- Merge origin/main into request log payload policy by @ahstn
- Align payload policy OpenAPI limits by @ahstn

### Documentation
- Harden documentation graph and workflows by @ahstn
- Harden documentation graph and workflows by @ahstn in [#74](https://github.com/ahstn/oceans-llm/pull/74)
- Simplify canonical page metadata by @ahstn
- *(observability)* Codify issue-54 runtime contract by @ahstn
- *(observability)* Codify issue-54 runtime contract by @ahstn in [#76](https://github.com/ahstn/oceans-llm/pull/76)
- Adding images/screenshots by @ahstn
- *(adr)* Record admin logout decision by @ahstn

### Miscellaneous
- Updating gitignore by @ahstn




## [0.5.0] - 2026-03-29
### :rocket: New features
- *(ops)* Harden migrations and adopt pitchfork-first local postgres by @ahstn
- *(gateway)* Tighten accounting and request-log contracts by @ahstn
- *(gateway)* Tighten accounting and request-log contracts by @ahstn in [#55](https://github.com/ahstn/oceans-llm/pull/55)
- *(gateway)* Add budget threshold alerting by @ahstn
- *(gateway)* Add budget threshold alerting by @ahstn in [#58](https://github.com/ahstn/oceans-llm/pull/58)
- *(identity)* Harden admin lifecycle and team membership workflows by @ahstn in [#63](https://github.com/ahstn/oceans-llm/pull/63)
- *(gateway)* Add caller tags to request logs by @ahstn in [#62](https://github.com/ahstn/oceans-llm/pull/62)
- *(admin)* Generate live control-plane API contract by @ahstn
- *(admin)* Generate live control-plane API contract by @ahstn in [#72](https://github.com/ahstn/oceans-llm/pull/72)

### :bug: Bug fixes
- *(ci)* Skip postgres install in ci by @ahstn
- *(ci)* Skip postgres install in ci by @ahstn
- *(gateway)* Include budget id in alert dedupe by @ahstn
- *(identity)* Address review feedback after rebase by @ahstn
- *(admin)* Stabilize generated admin contract artifacts by @ahstn
- *(gateway)* Expose test metrics in debug builds by @ahstn
- *(observability)* Harden chat metrics and streamed request logging by @ahstn
- *(observability)* Harden chat metrics and streamed request logging by @ahstn in [#70](https://github.com/ahstn/oceans-llm/pull/70)
- *(observability)* Remove fallback-era request metadata by @ahstn
- *(gateway)* Drop duplicate stream error parsing by @ahstn
- *(gateway)* Finalize stream collector before success path by @ahstn
- *(store)* Guard postgres metadata cleanup migration by @ahstn
- *(observability)* Remove fallback-era request metadata by @ahstn in [#71](https://github.com/ahstn/oceans-llm/pull/71)

### Build
- Post release tasks - v0.4.0 by @ahstn

### Changed
- Refactor migration hook exposure and simplify local postgres guidance by @ahstn
- Merge branch 'main' into codex/post-success-accounting-request-log-contracts by @ahstn
- Merge branch 'main' into codex/issues-3-14-hardening-pitchfork by @ahstn
- *(gateway-store)* Harden migrations and simplify local postgres workflow by @ahstn in [#57](https://github.com/ahstn/oceans-llm/pull/57)
- Implement admin identity lifecycle hardening by @ahstn
- *(identity)* Tighten lifecycle boundaries by @ahstn
- Add request caller tags to observability by @ahstn
- *(observability)* Tighten request log tag filters by @ahstn
- *(main)* Integrate latest observability changes by @ahstn
- *(main)* Absorb latest observability cleanup by @ahstn

### Documentation
- Harden documentation graph by @ahstn
- Add contributing guide by @ahstn
- Expand canonical operator references by @ahstn
- Harden documentation graph by @ahstn in [#56](https://github.com/ahstn/oceans-llm/pull/56)
- *(adr)* Record identity lifecycle hardening by @ahstn
- *(adr)* Expand request log caller tag decision record by @ahstn

### Miscellaneous
- *(version)* V0.5.0 by @ahstn

### Testing
- *(admin-ui)* Cover trimmed request log tag filters by @ahstn




## [0.4.0] - 2026-03-17
### :rocket: New features
- *(admin-ui)* Add team management flow by @ahstn
- *(auth)* Add bootstrap admin login flow by @ahstn
- *(identity)* Add user signup and onboarding flow by @ahstn in [#12](https://github.com/ahstn/oceans-llm/pull/12)
- *(admin-ui)* Add team management flow by @ahstn in [#13](https://github.com/ahstn/oceans-llm/pull/13)
- *(deploy)* Add local and GHCR compose stacks by @ahstn
- *(deploy)* Add local and GHCR compose stacks by @ahstn in [#15](https://github.com/ahstn/oceans-llm/pull/15)
- *(gateway)* Add postgres runtime backend by @ahstn
- *(gateway)* Harden store migrations and runtime cli by @ahstn
- *(gateway)* Harden store migrations and runtime cli by @ahstn
- *(gateway)* Support model aliases by @ahstn
- *(gateway)* Harden model alias resolution by @ahstn
- *(gateway)* Add durable usage ledger accounting by @ahstn
- *(gateway)* Add durable usage ledger accounting by @ahstn in [#41](https://github.com/ahstn/oceans-llm/pull/41)
- *(admin-ui)* Refresh theme shell and auth surfaces by @ahstn
- *(admin-ui)* Add identity empty states and share flows by @ahstn
- *(admin-ui)* Improve responsive data surfaces by @ahstn
- *(ui)* Updating requests logs page by @ahstn
- *(admin-ui)* Refresh admin control plane surfaces by @ahstn in [#42](https://github.com/ahstn/oceans-llm/pull/42)
- *(gateway)* Enforce capability-aware routing before provider execution by @ahstn
- *(gateway)* Complete provider-neutral core boundary and capability-aware routing by @ahstn in [#43](https://github.com/ahstn/oceans-llm/pull/43)
- *(gateway)* Support model aliases by @ahstn in [#40](https://github.com/ahstn/oceans-llm/pull/40)
- *(gateway)* Close embeddings and openai-compat streaming runtime gaps by @ahstn
- *(gateway)* Simplify v1 runtime routing and streaming by @ahstn
- *(gateway)* Simplify v1 runtime routing and streaming by @ahstn in [#47](https://github.com/ahstn/oceans-llm/pull/47)
- *(spend)* Ship spend reporting and team budget controls by @ahstn
- *(spend)* Deliver live spend reporting and team budget controls by @ahstn in [#48](https://github.com/ahstn/oceans-llm/pull/48)
- Complete observability foundations by @ahstn
- *(observability)* Complete runtime metrics and request-log evolution by @ahstn in [#51](https://github.com/ahstn/oceans-llm/pull/51)

### :bug: Bug fixes
- *(gateway)* Restore lint and test green by @ahstn
- *(gateway)* Restore lint and test green by @ahstn in [#36](https://github.com/ahstn/oceans-llm/pull/36)
- *(e2e)* Resolve mise from environment by @ahstn
- *(gateway)* Default maintenance task config by @ahstn
- *(smoke)* Make test task shell-compatible by @ahstn
- *(postgres)* Correct migration status lookup by @ahstn
- *(smoke)* Check gateway port by @ahstn
- *(gateway)* Address alias edge cases and CI regressions by @ahstn
- *(gateway-store)* Cast Postgres spend sums to bigint by @ahstn
- *(ci)* Provide dummy secondary OpenAI key for smoke runs by @ahstn
- *(providers)* Enforce payload-aware done handling in SSE by @ahstn
- *(gateway)* Satisfy clippy self convention for operation labels by @ahstn
- *(admin-ui)* Align page composition and copy across control plane by @ahstn
- *(spend)* Enforce hard limits before provider calls by @ahstn
- Add owner indexes for request logs by @ahstn
- *(ci)* Satisfy lint and harden request logging by @ahstn
- *(ci)* Restore chat log metadata and migration assertions by @ahstn

### Build
- Disable ARM builds until we have better gha runners by @ahstn
- Post release tasks - v0.2.0 by @ahstn
- Post release tasks - v0.3.0 by @ahstn
- Adding worktrunk config by @ahstn
- Add pre-commit for linting and file hygiene by @ahstn

### Changed
- Implement user signup and onboarding flow by @ahstn
- Fix local admin UI gateway routing by @ahstn
- Merge origin/main into feat/team-creation by @ahstn
- *(ui)* Request log table padding fixes by @ahstn
- *(gateway)* Decouple provider execution from OpenAI DTOs by @ahstn
- Merge origin/main into codex/model-aliases by @ahstn
- Preserve observability response metadata by @ahstn

### Documentation
- Adding adr by @ahstn
- *(adr)* Record capability-aware route gating decision by @ahstn

### Miscellaneous
- Post release tasks by @ahstn
- Removing old semantic release setup by @ahstn
- Update mise config by @ahstn
- Resolve conflicts by @ahstn
- *(version)* V0.4.0 by @ahstn

### Testing
- *(admin-ui)* Add end-to-end contract harness by @ahstn
- *(admin-ui)* Add end-to-end contract harness by @ahstn in [#37](https://github.com/ahstn/oceans-llm/pull/37)




## [0.1.0] - 2026-03-08
### :rocket: New features
- Initial commit by @ahstn
- Add admin-ui crate with tanstack start control plane by @ahstn
- Add admin-ui crate with TanStack Start control plane shell by @ahstn in [#1](https://github.com/ahstn/oceans-llm/pull/1)
- *(gateway)* Add foundational API, service, store, and provider crates by @ahstn
- *(gateway)* Implement vertex-first chat provider foundation by @ahstn
- *(gateway)* Add Vertex-first chat execution foundation by @ahstn in [#10](https://github.com/ahstn/oceans-llm/pull/10)

### Build
- *(release)* Simplify release pipeline around cocogitto and git-cliff by @ahstn in [#11](https://github.com/ahstn/oceans-llm/pull/11)

### CI
- Add rust workflow and ui-check task by @ahstn
- Add rust workflow and enforce ui-install via mise by @ahstn in [#6](https://github.com/ahstn/oceans-llm/pull/6)

### Changed
- Fix admin UI upstream loopback and restore Tailwind styling by @ahstn
- Fix admin UI local proxy reliability and Tailwind rendering by @ahstn in [#2](https://github.com/ahstn/oceans-llm/pull/2)
- Implement identity and user management foundation by @ahstn
- Harden budget accounting precision and policy docs by @ahstn
- Identity foundation and budget accounting hardening by @ahstn in [#5](https://github.com/ahstn/oceans-llm/pull/5)
- Implement request logging and Vertex stream guards by @ahstn
- Add hybrid pricing catalog support by @ahstn
- Fix Vertex stream decoding and terminal state by @ahstn

### Documentation
- Add issue template and gh workflow reminders by @ahstn
- Split issue templates into feature and bug forms by @ahstn
- Add dedicated feature/bug issue templates by @ahstn in [#9](https://github.com/ahstn/oceans-llm/pull/9)
- *(adr)* Add attribution note to vertex foundation ADR by @ahstn
- Updating documentation by @ahstn
- Updating documentation by @ahstn

### Miscellaneous
- Add pull request template by @ahstn
- Adding fallback pricing data by @ahstn
- *(version)* V0.1.0 by @ahstn

### Testing
- *(vertex)* Harden stream parsing and add adapter HTTP tests by @ahstn




[0.6.0]: https://github.com/ahstn/oceans-llm/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/ahstn/oceans-llm/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/ahstn/oceans-llm/compare/v0.1.0...v0.4.0
[0.1.0]: https://github.com/ahstn/oceans-llm/tree/v0.1.0

<!-- generated by git-cliff -->
