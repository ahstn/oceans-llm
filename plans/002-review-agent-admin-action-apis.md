# Plan 002: Review Agent Admin and Action APIs

Issue: #194
Depends on: [001-review-agent-persistence.md](001-review-agent-persistence.md)
Suggested branch: `codex/review-agent-apis`

## Goal

Expose the Review Agent control plane through admin APIs and the GitHub Action
runtime APIs. The admin side is session-authenticated and operator/team scoped.
The action side is authenticated with existing Oceans service account API keys.

## Route Shape

Admin routes:

- `GET /api/v1/admin/review-agent/repositories`
- `POST /api/v1/admin/review-agent/repositories`
- `GET /api/v1/admin/review-agent/repositories/{id}`
- `PATCH /api/v1/admin/review-agent/repositories/{id}`
- `POST /api/v1/admin/review-agent/repositories/{id}/disable`
- `POST /api/v1/admin/review-agent/repositories/{id}/reactivate`
- `GET /api/v1/admin/review-agent/repositories/{id}/runs`
- `POST /api/v1/admin/review-agent/repositories/{id}/workflow`

Action routes:

- `POST /api/v1/review-agent/action/config/resolve`
- `POST /api/v1/review-agent/action/runs`
- `POST /api/v1/review-agent/action/runs/{id}/heartbeat`
- `POST /api/v1/review-agent/action/runs/{id}/complete`
- `POST /api/v1/review-agent/action/runs/{id}/fail`

Register routes in `crates/gateway/src/http/mod.rs`.

## Authorization Model

Admin:

- Platform admins can manage all Review Agent repositories.
- Team owners/admins can manage repositories bound to service accounts in their
  teams.
- Team members cannot manage Review Agent repositories.
- Reuse patterns from `crates/gateway/src/http/identity.rs` and
  `crates/gateway/src/http/api_keys.rs` for team manager checks.

Action:

- Authenticate bearer tokens with the existing gateway API key authenticator.
- Require an active API key owned by a service account.
- Require the configured repository's `service_account_id` to match the
  authenticated service account.
- Validate provider and repo identity before resolving config or accepting run
  reports.
- Reject disabled/archived repositories.

Do not redesign `/v1` auth. The same service account API key can be used by the
action for Oceans-backed OpenAI-compatible model calls.

## Service Layer

Add `crates/gateway-service/src/review_agent.rs` and expose it from the service
crate. Keep HTTP handlers thin.

Service responsibilities:

- admin CRUD/status changes for configured repositories
- admin run listing
- generated workflow rendering
- action config resolution
- action run start, heartbeat, complete, and fail lifecycle
- same-repo/non-draft PR safety checks
- service-account binding checks
- config precedence and rejected override reporting
- sanitized metrics validation

Config precedence:

1. Safety invariants
2. Per-run action inputs
3. Oceans repository defaults

Required safety invariants:

- only `pull_request` events
- same-repo PR head and base
- non-draft PR
- active configured repository
- authenticated service account bound to that repository
- feature toggles cannot override safety checks

## DTOs and Validation

Use typed request/response structs. For action reporting request DTOs, use
`#[serde(deny_unknown_fields)]` so raw artifacts, prompts, diffs, or model
output fields are rejected instead of silently ignored.

Key response shapes:

- config resolve returns effective config, applied overrides, rejected
  overrides, model execution mode, provider/model keys, Oceans base URL for
  Oceans-backed mode, and reporting hints.
- run start returns `run_id`, current status, and reporting endpoints.
- completion/failure accepts sanitized metrics and short error summaries only.
- workflow generation returns YAML and secret/input placeholders, never secret
  values.

HTTP semantics:

- `401`: missing or invalid action bearer token
- `403`: authenticated but not allowed for this repository/team
- `404`: repository or run not visible to caller
- `409`: disabled repo, duplicate active repo, invalid lifecycle transition
- `422`: malformed config, failed safety invariant, invalid reporting payload

## OpenAPI Contract

The gateway HTTP layer is the contract source of truth. Update
`crates/gateway/src/http/admin_contract.rs` to include both admin and action
paths.

Add a distinct security scheme for action bearer auth if the current contract
only documents session cookies.

Regenerate and verify:

```sh
eval "$(/Users/ahstn/.local/bin/mise activate zsh)"
mise run admin-contract-generate
mise run admin-contract-check
```

Update `crates/admin-ui/web/src/types/live-api.ts` with the generated Review
Agent type aliases. Full admin UI screens can remain a later issue, but server
client functions for workflow generation and repository listing are useful
scaffolding if #194 callers need them immediately.

## Workflow Generation

The generated YAML should include:

- `on: pull_request`
- same-repo guard
- draft skip guard
- least-privilege permissions for contents, pull requests, and issues only as
  needed
- concurrency group with cancellation
- checkout of the PR head SHA
- `uses: ahstn/oceans-llm/actions/review-agent@<ref>`
- placeholders for `oceans-url` and `oceans-api-key`
- documented optional inputs for model and feature toggles

Never include API key values, provider secrets, or local-only URLs unless the
caller explicitly supplies them as placeholders.

## Tests

Service tests:

- config merge precedence
- rejected override reporting
- service account binding checks
- same-repo and draft validation
- workflow YAML rendering
- lifecycle transitions and idempotency around GitHub run ID/attempt
- sanitized completion/failure payload validation

HTTP tests:

- platform admin can manage all configured repos
- team owner/admin can manage repos bound to their team's service accounts
- team member is rejected
- action endpoint rejects missing, invalid, revoked, user-owned, wrong-team, and
  wrong-service-account API keys
- disabled/archived repos reject config resolution and run start
- reporting payloads with unknown raw fields are rejected
- status code mapping matches #194 semantics

Contract tests:

- OpenAPI includes all admin and action paths
- generated TypeScript types compile
- admin contract check passes after generation

## Verification

```sh
eval "$(/Users/ahstn/.local/bin/mise activate zsh)"
mise run admin-contract-generate
mise run admin-contract-check
cargo test -p gateway-service review_agent
cargo test -p gateway review_agent
mise run lint
```

If the implementation adds UI server helpers:

```sh
bun test --cwd crates/admin-ui/web
bun run --cwd crates/admin-ui/web lint
```
