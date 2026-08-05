# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/2.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [0.21.0] - 2026-08-07
### :rocket: New features
- *(observability)* Add configurable OTLP trace sampling by @ahstn
- *(observability)* Add configurable OTLP trace sampling by @ahstn in [#268](https://github.com/ahstn/oceans-llm/pull/268)

### :bug: Bug fixes
- *(observability)* Preserve remote trace context by @ahstn
- *(admin-ui)* Render harness icon masks by @ahstn
- *(admin-ui)* Render harness icon masks by @ahstn in [#269](https://github.com/ahstn/oceans-llm/pull/269)

### Build
- Post release tasks - v0.20.1 by @ahstn

### Miscellaneous
- *(version)* V0.21.0

### Testing
- *(admin-ui)* Verify harness mask properties by @ahstn




## [0.20.1] - 2026-08-05
### Fixed
- *(helm)* Validate gateway config before migration wait by @ahstn
- *(gateway)* Validate server configuration by @ahstn
- *(admin-ui)* Address page header review feedback by @ahstn

### Build
- Post release tasks - v0.20.0 by @ahstn

### Miscellaneous
- *(deps)* Minor version upgrades by @ahstn
- *(version)* V0.20.1 by @ahstn

### Styling
- *(admin-ui)* Add page headers by @ahstn
- *(admin-ui)* Add page headers by @ahstn in [#264](https://github.com/ahstn/oceans-llm/pull/264)



## [0.20.0] - 2026-08-05
### Added
- *(admin-ui)* Add agent harness icons by @ahstn
- *(mcp)* Add Google Workspace OAuth connections by @ahstn
- *(auth)* Add regular-user self-service access by @ahstn
- *(identity)* Add read-only user directories by @ahstn
- *(admin-ui)* Redesign authentication flows by @ahstn

### Fixed
- *(admin-ui)* Tighten models table layout by @ahstn
- *(admin-ui)* Polish models table controls by @ahstn
- *(admin-ui)* Compact model allow list by @ahstn
- *(mcp)* Harden OAuth connection lifecycle by @ahstn
- *(ui)* Satisfy React Doctor checks by @ahstn
- *(mcp)* Address security review findings by @ahstn
- *(mcp)* Address OAuth review feedback by @ahstn
- *(admin)* Harden regular-user access by @ahstn
- *(admin-ui)* Simplify account ready guidance by @ahstn
- *(admin-ui)* Address authentication review findings by @ahstn
- *(admin-ui)* Complete SSO redirect feedback by @ahstn




## [0.19.0] - 2026-07-20
### Added
- Initial mise.toml by @ahstn
- *(build)* Adding react doctor by @ahstn
- *(build)* Enabling stricter react oxlint rules by @ahstn
- *(build)* Fix oxfmt by @ahstn

### Fixed
- *(harness)* Address integration review findings by @ahstn
- *(harness)* Address security and contract review by @ahstn
- *(harness)* Disable Pi startup network checks by @ahstn
- *(providers)* Harden upstream compatibility by @ahstn
- *(providers)* Address compatibility review by @ahstn
- *(providers)* Resolve automated review findings by @ahstn
- *(bedrock)* Revalidate merged Converse controls by @ahstn
- *(openai-compat)* Normalize empty tool choice by @ahstn
- *(gateway)* Gate chat file inputs by capability by @ahstn




## [0.18.0] - 2026-07-17
### Added
- *(admin-ui)* Show running Oceans version by @ahstn
- *(gateway)* Support route metadata overrides by @ahstn

### Fixed
- *(admin-ui)* Bound runtime version lookup by @ahstn
- *(gateway)* Preserve route metadata semantics by @ahstn
- *(gateway)* Gate degraded client configs by @ahstn
- *(gateway)* Retain regional model limits by @ahstn
- *(gateway)* Align metadata pricing eligibility by @ahstn
- *(gateway)* Skip inactive route context validation by @ahstn
- *(ci)* Correct locked Helm download URLs by @ahstn
- *(ci)* Run contract verification with Bash by @ahstn
- *(ci)* Address workflow review findings by @ahstn




## [0.17.0] - 2026-07-14
### Added
- *(identity)* Seed config tags for identity entities by @ahstn
- Update catalog pricing by @ahstn

### Changed
- *(pricing)* Simplify catalog test setup by @ahstn

### Fixed
- *(gateway)* Refresh model pricing catalog by @ahstn
- *(gateway)* Harden pricing refresh sync by @ahstn
- *(identity)* Share tag validation for config tags by @ahstn
- *(identity)* Address config tag review feedback by @ahstn
- Update client config generation and onboarding reset links by @ahstn
- Narrow onboarding reset updates by @ahstn
- Address client config review feedback by @ahstn
- Address follow-up PR review comments by @ahstn
- Address onboarding reset review feedback by @ahstn
- *(pricing)* Make catalog reconciliation race-safe by @ahstn
- *(pricing)* Address reconciliation review findings by @ahstn
- *(pricing)* Allocate catalog generations atomically by @ahstn
- *(pricing)* Reject stale catalog insertions by @ahstn
- *(pricing)* Preserve concurrent catalog winner by @ahstn
- *(pricing)* Harden catalog refresh convergence by @ahstn




## [0.16.0] - 2026-07-07
### Added
- *(models)* Updating model icons by @ahstn

### Changed
- Add admin root redirect by @ahstn
- Fix gateway root admin redirect by @ahstn
- Improve models table controls by @ahstn
- Widen model info dialog layout by @ahstn
- Refine models table layout by @ahstn
- Reposition model actions column by @ahstn

### Fixed
- *(config)* Preserve demo model access by @ahstn




## [0.15.0] - 2026-07-06
### Added
- *(gateway)* Add declarative user budget defaults by @ahstn

### Changed
- Fix admin muted surface and API key dialog layout by @ahstn
- Add service account API key reveal controls by @ahstn

### Fixed
- *(gateway)* Harden budget default reconciliation by @ahstn
- *(gateway)* Preserve budget deactivation guards by @ahstn




## [0.14.0] - 2026-07-06
### Added
- *(codex)* Add Bedrock-safe client config by @ahstn
- *(dev)* Adding impeccable artifacts by @ahstn
- *(review)* Review agent first pass by @ahstn
- *(ui)* Migrate to blue/ocean focus color palette by @ahstn
- *(ui)* Initial service account draft by @ahstn
- Add native Vertex embeddings by @ahstn
- Add gemini embedding 2 support by @ahstn
- *(gateway)* Add model-level allowlists by @ahstn

### Changed
- Add GNU Affero General Public License v3 by @ahstn

### Fixed
- *(bedrock)* Reject forced image generation tools by @ahstn
- *(codex)* Tighten generated config and tool guards by @ahstn
- *(admin-ui)* Refine review agent dialog layout by @ahstn
- *(admin-ui)* Address review agent PR feedback by @ahstn
- *(admin-ui)* Serve static assets before auth fallback by @ahstn
- *(admin-ui)* Address static asset review findings by @ahstn
- Address Vertex embeddings review feedback by @ahstn
- Address embedding review comments by @ahstn
- *(gateway)* Address model allowlist review comments by @ahstn




## [0.13.0] - 2026-07-03
### Added
- *(gateway)* Allow api keys to grant all models by @ahstn
- *(gateway)* Add declarative service account config by @ahstn
- *(admin)* Add client config setup context by @ahstn

### Changed
- Align docs navigation with admin UI by @ahstn
- Polish docs sidebar layout by @ahstn
- Split docs navigation surfaces by @ahstn
- Refine docs getting started index by @ahstn
- Simplify getting started docs index by @ahstn

### Fixed
- *(test)* Repair api key all-model ci coverage by @ahstn
- *(gateway)* Address api key grant review feedback by @ahstn
- *(admin)* Refresh pricing before releases by @ahstn
- *(gateway)* Harden service account key handling by @ahstn
- *(e2e)* Seed service account config with new shape by @ahstn




## [0.12.0] - 2026-07-01
### Added
- *(gateway)* Implement oidc sso by @ahstn
- *(auth)* Add direct GitHub OAuth SSO by @ahstn
- *(gateway)* Export spend as FOCUS CSV by @ahstn
- *(gateway)* Include owner tags in FOCUS export by @ahstn
- *(gateway)* Add external MCP registry by @ahstn
- *(gateway)* Add MCP gateway auth and diagnostics by @ahstn
- *(gateway)* Implement budget principal taxonomy by @ahstn
- *(gateway)* Add bedrock mantle support by @ahstn
- *(gateway)* Track additional agent harness user agents by @ahstn
- *(gateway)* Add aggregate MCP discovery endpoint by @ahstn
- *(gateway)* Add MCP credential execution by @ahstn
- *(admin-ui)* Add Claude Code client config snippets by @ahstn
- Use central cargo build dirs by @ahstn
- *(admin-ui)* Redesign request log detail as wide inspect drawer by @ahstn
- *(gateway)* Re-anchor local demo seed on every run with richer fixtures by @ahstn
- *(gateway)* Add OpenRouter routing policy controls by @ahstn
- *(admin-ui)* Surface api key and caller in the request logs table by @ahstn
- *(gateway)* Support Cloud Run OpenAI-compatible providers by @ahstn
- *(gateway)* Restrict github oauth email domains by @ahstn
- *(gateway)* Add Codex client config by @ahstn
- *(gateway)* Add Anthropic messages for Vertex Claude tools by @ahstn
- Add multi-model client config generation by @ahstn
- *(review-agent)* Add GitHub review agent foundation by @ahstn
- *(client-config)* Add Fable 5 adaptive config by @ahstn

### Changed
- Add hosted MCP recommendations by @ahstn
- Record MCP gateway auth alignment interview by @ahstn
- *(providers)* Share openai stream normalization by @ahstn
- Implement MCP grants and token overhead telemetry by @ahstn
- Address MCP PR review findings by @ahstn
- *(gateway)* Simplify oauth domain policy helpers by @ahstn
- Polish admin MCP workspace UI by @ahstn
- Refine admin MCP server cards by @ahstn
- Align admin identity and MCP tables by @ahstn
- Polish MCP tools dialog by @ahstn
- Document MCP admin workflows by @ahstn
- *(client-config)* Split client renderers by concern by @ahstn

### Fixed
- *(gateway)* Harden oidc review findings by @ahstn
- *(gateway)* Address oidc review hardening by @ahstn
- *(auth)* Address GitHub OAuth PR feedback by @ahstn
- *(auth)* Make SSO JIT retries recoverable by @ahstn
- *(auth)* Harden OAuth provider updates by @ahstn
- *(gateway)* Address FOCUS export review findings by @ahstn
- *(gateway)* Address FOCUS export review follow-ups by @ahstn
- *(gateway)* Harden MCP registry discovery by @ahstn
- *(gateway)* Address MCP registry review feedback by @ahstn
- *(gateway)* Tighten MCP catalog auth overrides by @ahstn
- *(gateway)* Handle MCP discovery pagination by @ahstn
- *(gateway)* Skip non-response MCP SSE events by @ahstn
- *(ci)* Prebuild gateway for e2e stack by @ahstn
- *(admin-ui)* Prevent pre-hydration auth submits by @ahstn
- *(gateway)* Harden MCP proxy handling by @ahstn
- *(gateway)* Address MCP PR review feedback by @ahstn
- Address budget taxonomy PR feedback by @ahstn
- Resolve budget review edge cases by @ahstn
- Bind libsql budget query parameters conditionally by @ahstn
- *(gateway)* Tighten bedrock responses routing by @ahstn
- *(gateway)* Validate bedrock route capabilities by @ahstn
- *(e2e)* Seed service account in gateway config by @ahstn
- *(gateway)* Harden MCP credential resolution by @ahstn
- *(gateway)* Address MCP PR review findings by @ahstn
- *(client-config)* Correct Claude Code gateway settings by @ahstn
- *(gateway)* Harden OpenRouter policy validation by @ahstn
- *(gateway)* Keep Cloud Run auth inside provider boundary by @ahstn
- Fix cloud run provider review findings by @ahstn
- *(gateway)* Handle oauth review followups by @ahstn
- *(admin-ui)* Address MCP review findings by @ahstn
- *(gateway)* Refine Codex config availability by @ahstn
- *(gateway)* Preserve Anthropic stream errors and usage by @ahstn
- *(client-config)* Use Anthropic APIs for Claude models by @ahstn
- *(gateway)* Address Anthropic messages PR review findings by @ahstn
- *(gateway)* Align Messages errors and config API inference by @ahstn
- *(gateway)* Include usage in Anthropic stream deltas by @ahstn
- *(gateway)* Harden Anthropic stream usage accounting by @ahstn
- *(auth)* Improve github oauth email verification handling by @ahstn
- *(auth)* Address github oauth review feedback by @ahstn
- Address client config review findings by @ahstn
- *(review-agent)* Address PR review findings by @ahstn
- *(providers)* Align adaptive Claude policy by @ahstn
- *(providers)* Address adaptive Claude review by @ahstn




## [0.8.0] - 2026-05-15
### Added
- *(deploy)* Add Helm OCI chart by @ahstn
- *(gateway)* Add request-attempt observability by @ahstn
- *(gateway)* Add bedrock streaming and claude support by @ahstn
- *(gateway)* Support bedrock aws credential chain by @ahstn
- *(providers)* Add claude thinking compatibility by @ahstn
- *(admin)* Add Anthropic client config snippets by @ahstn
- *(observability)* Add tool cardinality request logs by @ahstn
- *(observability)* Add agent harness usage by @ahstn
- *(admin-ui)* Show request operations in logs by @ahstn
- *(observability)* Add MCP invocation audit logs by @ahstn
- *(gateway)* Add request log retention purge by @ahstn
- *(gateway)* Add team service accounts by @ahstn
- *(gateway)* Add identity entity tags by @ahstn
- *(admin-ui)* Add expandable team rows by @ahstn
- Updating icons and sidebar nav by @ahstn

### Changed
- *(deploy)* Split Helm hook jobs by @ahstn
- Add AWS Bedrock provider and Converse chat support by @ahstn
- Improve container runtime hardening and admin errors by @ahstn
- Polish admin UI shell by @ahstn
- Refine admin sidebar navigation by @ahstn
- Restore inset shell border by @ahstn
- Theme native admin scrollbars by @ahstn
- Render OpenAI brand icon inline by @ahstn
- Expand local demo seed data by @ahstn
- *(gateway)* Split local demo seed fixtures by @ahstn
- Address harness usage PR review findings by @ahstn
- *(admin-ui)* Simplify request operation label rendering by @ahstn
- Polish teams member toggle by @ahstn
- Add generated avatars and user detail dialog by @ahstn
- Polish user details dialog by @ahstn

### Fixed
- *(deploy)* Address Helm review feedback by @ahstn
- *(gateway)* Sanitize request-attempt error details by @ahstn
- *(gateway)* Stabilize admin contract checks by @ahstn
- *(gateway)* Satisfy vertex stream clippy by @ahstn
- *(gateway)* Address bedrock review feedback by @ahstn
- *(providers)* Validate native claude effort fields by @ahstn
- *(providers)* Require bedrock converse thinking budgets by @ahstn
- *(providers)* Validate vertex anthropic overrides by @ahstn
- *(providers)* Tighten anthropic thinking validation by @ahstn
- *(observability)* Address tool cardinality review findings by @ahstn
- Correcting helm lint issue by @ahstn
- Correcting helm mise command by @ahstn
- *(admin-ui)* Address review feedback by @ahstn
- *(observability)* Tighten MCP invocation logging by @ahstn
- *(observability)* Address PR review findings by @ahstn
- *(gateway)* Address purge review findings by @ahstn
- *(gateway)* Reconcile request-log purge rebase by @ahstn
- *(gateway)* Address service account review findings by @ahstn
- *(gateway-store)* Address service account review feedback by @ahstn
- *(gateway)* Integrate service accounts with main by @ahstn
- *(gateway)* Reconcile service accounts with latest main by @ahstn
- *(gateway)* Surface models.dev pricing metadata by @ahstn
- *(gateway)* Address pricing catalog review feedback by @ahstn
- Address identity tag review comments by @ahstn




## [0.6.0] - 2026-04-24
### Added
- *(docs)* Publish docs site with vitepress by @ahstn
- *(gateway)* Add declarative config seeding for teams and users by @ahstn
- *(admin-ui)* Adopt shadcn sidebar preset layout by @ahstn
- *(gateway)* Seed richer local demo data by @ahstn
- *(admin-ui)* Add provider and model brand icons by @ahstn
- *(admin)* Improve provider branding and lookup efficiency by @ahstn
- *(admin-ui)* Improve models page table scrolling by @ahstn
- *(models)* Updating models api by @ahstn
- *(gateway)* Add provider compatibility profiles by @ahstn
- *(admin)* Add current-session logout by @ahstn
- *(gateway)* Add OpenAI Responses API support by @ahstn

### Changed
- Implement live admin API key management by @ahstn
- *(api-keys)* Harden admin lifecycle architecture by @ahstn
- *(gateway-store)* Rebaseline pre-v1 migrations by @ahstn
- Fix declarative seed validation ordering by @ahstn
- Simplify local runtime setup with mise by @ahstn
- Polish API key management flows by @ahstn
- Add observability usage leaderboard by @ahstn
- Normalize generated admin API typings by @ahstn
- Fix admin UI localhost SSR auth flow by @ahstn
- Harden request log payload policy by @ahstn
- Align payload policy OpenAPI limits by @ahstn

### Fixed
- *(api-keys)* Address rebase fallout and review findings by @ahstn
- *(gateway)* Normalize declarative identity config values by @ahstn
- *(gateway-store)* Guard seeded identity auth mutations by @ahstn
- *(admin)* Paginate models and redact provider cache by @ahstn
- *(gateway)* Keep local demo bootstrap-safe by @ahstn
- *(admin-ui)* Restore upstream model column layout by @ahstn
- *(ui)* Fixing overscroll on main body content by @ahstn




## [0.5.0] - 2026-03-29
### Added
- *(ops)* Harden migrations and adopt pitchfork-first local postgres by @ahstn
- *(gateway)* Tighten accounting and request-log contracts by @ahstn
- *(gateway)* Add budget threshold alerting by @ahstn
- *(admin)* Generate live control-plane API contract by @ahstn

### Changed
- Refactor migration hook exposure and simplify local postgres guidance by @ahstn
- Implement admin identity lifecycle hardening by @ahstn
- *(identity)* Tighten lifecycle boundaries by @ahstn
- Add request caller tags to observability by @ahstn
- *(observability)* Tighten request log tag filters by @ahstn

### Fixed
- *(ci)* Skip postgres install in ci by @ahstn
- *(gateway)* Include budget id in alert dedupe by @ahstn
- *(identity)* Address review feedback after rebase by @ahstn
- *(admin)* Stabilize generated admin contract artifacts by @ahstn
- *(gateway)* Expose test metrics in debug builds by @ahstn
- *(observability)* Harden chat metrics and streamed request logging by @ahstn
- *(observability)* Remove fallback-era request metadata by @ahstn
- *(gateway)* Drop duplicate stream error parsing by @ahstn
- *(gateway)* Finalize stream collector before success path by @ahstn
- *(store)* Guard postgres metadata cleanup migration by @ahstn




## [0.4.0] - 2026-03-17
### Added
- *(admin-ui)* Add team management flow by @ahstn
- *(auth)* Add bootstrap admin login flow by @ahstn
- *(identity)* Add user signup and onboarding flow (#12) by @ahstn in [#12](https://github.com/ahstn/oceans-llm/pull/12)
- *(deploy)* Add local and GHCR compose stacks by @ahstn
- *(gateway)* Add postgres runtime backend by @ahstn
- *(gateway)* Harden store migrations and runtime cli by @ahstn
- *(gateway)* Support model aliases by @ahstn
- *(gateway)* Harden model alias resolution by @ahstn
- *(gateway)* Add durable usage ledger accounting by @ahstn
- *(admin-ui)* Refresh theme shell and auth surfaces by @ahstn
- *(admin-ui)* Add identity empty states and share flows by @ahstn
- *(admin-ui)* Improve responsive data surfaces by @ahstn
- *(ui)* Updating requests logs page by @ahstn
- *(gateway)* Enforce capability-aware routing before provider execution by @ahstn
- *(gateway)* Close embeddings and openai-compat streaming runtime gaps by @ahstn
- *(gateway)* Simplify v1 runtime routing and streaming by @ahstn
- *(spend)* Ship spend reporting and team budget controls by @ahstn
- Complete observability foundations by @ahstn

### Changed
- Implement user signup and onboarding flow by @ahstn
- Fix local admin UI gateway routing by @ahstn
- *(ui)* Request log table padding fixes by @ahstn
- *(gateway)* Decouple provider execution from OpenAI DTOs by @ahstn
- Preserve observability response metadata by @ahstn

### Fixed
- *(gateway)* Restore lint and test green by @ahstn
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




## [0.1.0] - 2026-03-08
### Added
- Initial commit by @ahstn
- Add admin-ui crate with tanstack start control plane by @ahstn
- *(gateway)* Add foundational API, service, store, and provider crates by @ahstn
- *(gateway)* Implement vertex-first chat provider foundation by @ahstn

### Changed
- Fix admin UI upstream loopback and restore Tailwind styling by @ahstn
- Implement identity and user management foundation by @ahstn
- Harden budget accounting precision and policy docs by @ahstn
- Implement request logging and Vertex stream guards by @ahstn
- Add hybrid pricing catalog support by @ahstn
- Fix Vertex stream decoding and terminal state by @ahstn




[0.21.0]: https://github.com/ahstn/oceans-llm/compare/v0.20.1...v0.21.0
[0.20.1]: https://github.com/ahstn/oceans-llm/compare/v0.20.0...v0.20.1
[0.20.0]: https://github.com/ahstn/oceans-llm/compare/v0.19.0...v0.20.0
[0.19.0]: https://github.com/ahstn/oceans-llm/compare/v0.18.0...v0.19.0
[0.18.0]: https://github.com/ahstn/oceans-llm/compare/v0.17.0...v0.18.0
[0.17.0]: https://github.com/ahstn/oceans-llm/compare/v0.16.0...v0.17.0
[0.16.0]: https://github.com/ahstn/oceans-llm/compare/v0.15.0...v0.16.0
[0.15.0]: https://github.com/ahstn/oceans-llm/compare/v0.14.0...v0.15.0
[0.14.0]: https://github.com/ahstn/oceans-llm/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/ahstn/oceans-llm/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/ahstn/oceans-llm/compare/v0.8.0...v0.12.0
[0.8.0]: https://github.com/ahstn/oceans-llm/compare/v0.6.0...v0.8.0
[0.6.0]: https://github.com/ahstn/oceans-llm/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/ahstn/oceans-llm/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/ahstn/oceans-llm/compare/v0.1.0...v0.4.0
[0.1.0]: https://github.com/ahstn/oceans-llm/tree/v0.1.0

<!-- generated by git-cliff -->
