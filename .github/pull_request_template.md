## Description

A clear and concise description of this PR.

PR title format must follow Conventional Commits, for example `feat(gateway): add release workflow`.

Use this section for review hints, explanations, discussion points, and follow-up TODOs.

- Summary of changes
- Why this approach was chosen
- How it works
- Risks, tradeoffs, and alternatives considered
- Additional context for reviewers

## Release Readiness Checklist

- [ ] `mise run lint`
- [ ] `mise run test`
- [ ] If this PR touches runtime, store, migration, or release behavior: `mise run check-rust-postgres`
- [ ] If this PR touches runtime, store, migration, or release behavior: `mise run test-rust-postgres`
- [ ] If this PR touches runtime, store, migration, or release behavior: `mise run test-gateway-postgres-smoke`
