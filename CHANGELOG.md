# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


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
- Resolve conflicts
- *(version)* V0.4.0

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




[0.4.0]: https://github.com/ahstn/oceans-llm/compare/v0.1.0...v0.4.0
[0.1.0]: https://github.com/ahstn/oceans-llm/tree/v0.1.0

<!-- generated by git-cliff -->
