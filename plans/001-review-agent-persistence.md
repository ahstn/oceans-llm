# Plan 001: Review Agent Persistence Model

Issue: #193
Depends on: none
Suggested branch: `codex/review-agent-persistence`

## Goal

Add the durable Review Agent storage layer for configured repositories, pull
requests, and review runs. This should be metadata-first, provider-neutral, and
ready for the admin/action APIs in #194.

## Scope

Create three MVP tables:

1. `review_agent_repositories`
2. `review_agent_pull_requests`
3. `review_agent_runs`

Do not add `review_agent_tokens`. Issue #194 explicitly chooses existing
service account API keys for MVP action auth, so repository configuration should
bind to `service_account_id` instead.

## Migration Work

Add shared migration version `V34` in both backends:

- `crates/gateway-store/migrations/V34__review_agent.sql`
- `crates/gateway-store/migrations/postgres/V34__review_agent.sql`
- register both in `crates/gateway-store/src/migration_registry.rs`

Follow `docs/reference/migration-authoring.md`: both backend schemas should be
logically equivalent and the registry version should stay sorted.

### `review_agent_repositories`

Recommended columns:

- `repository_id TEXT PRIMARY KEY`
- `provider TEXT NOT NULL CHECK (provider IN ('github'))`
- `external_repository_id TEXT NULL`
- `owner TEXT NOT NULL`
- `name TEXT NOT NULL`
- `full_name TEXT NOT NULL`
- `service_account_id TEXT NOT NULL REFERENCES service_accounts(service_account_id)`
- `status TEXT NOT NULL CHECK (status IN ('active', 'disabled', 'archived')) DEFAULT 'active'`
- `inline_review_enabled BOOLEAN/INTEGER NOT NULL DEFAULT true`
- `pr_summary_enabled BOOLEAN/INTEGER NOT NULL DEFAULT true`
- `diagrams_enabled BOOLEAN/INTEGER NOT NULL DEFAULT false`
- `linked_issue_detection_enabled BOOLEAN/INTEGER NOT NULL DEFAULT true`
- `linked_issue_assessment_enabled BOOLEAN/INTEGER NOT NULL DEFAULT false`
- `default_model_key TEXT NULL`
- `max_inline_comments INTEGER NULL`
- `request_changes_on_high_severity BOOLEAN/INTEGER NOT NULL DEFAULT false`
- `settings_json TEXT NULL`
- `created_at TEXT/TIMESTAMPTZ NOT NULL`
- `updated_at TEXT/TIMESTAMPTZ NOT NULL`

Indexes and constraints:

- Unique `(provider, external_repository_id)` when `external_repository_id IS NOT NULL`.
- Prevent duplicate active repos for `(provider, owner, name)` with a partial
  unique index where `status = 'active'`.
- Index `service_account_id` for admin authorization queries.

`default_model_key` should use the external model key that users and actions
understand. If implementation finds an existing stable model ID is preferred,
use that instead and document the choice in the PR.

### `review_agent_pull_requests`

Recommended columns:

- `pull_request_id TEXT PRIMARY KEY`
- `repository_id TEXT NOT NULL REFERENCES review_agent_repositories(repository_id)`
- `provider_pr_id TEXT NULL`
- `pr_number INTEGER NOT NULL`
- `title TEXT NULL`
- `author_login TEXT NULL`
- `state TEXT NOT NULL CHECK (state IN ('open', 'closed', 'merged', 'unknown'))`
- `head_sha TEXT NULL`
- `base_sha TEXT NULL`
- `head_repository_full_name TEXT NULL`
- `base_repository_full_name TEXT NULL`
- `is_draft BOOLEAN/INTEGER NOT NULL DEFAULT false`
- `created_at TEXT/TIMESTAMPTZ NOT NULL`
- `updated_at TEXT/TIMESTAMPTZ NOT NULL`

Indexes and constraints:

- Unique `(repository_id, pr_number)`.
- Index `(repository_id, state)`.

### `review_agent_runs`

Recommended columns:

- `run_id TEXT PRIMARY KEY`
- `repository_id TEXT NOT NULL REFERENCES review_agent_repositories(repository_id)`
- `pull_request_id TEXT NULL REFERENCES review_agent_pull_requests(pull_request_id)`
- `head_sha TEXT NULL`
- `github_run_id TEXT NULL`
- `github_run_attempt INTEGER NULL`
- `status TEXT NOT NULL CHECK (status IN ('queued', 'in_progress', 'succeeded', 'failed', 'cancelled', 'skipped'))`
- `started_at TEXT/TIMESTAMPTZ NULL`
- `heartbeat_at TEXT/TIMESTAMPTZ NULL`
- `finished_at TEXT/TIMESTAMPTZ NULL`
- `duration_ms INTEGER NULL`
- `files_changed INTEGER NULL`
- `additions INTEGER NULL`
- `deletions INTEGER NULL`
- `changed_loc INTEGER NULL`
- `inline_comments_created INTEGER NULL`
- `inline_comments_updated INTEGER NULL`
- `inline_comments_skipped INTEGER NULL`
- `managed_comment_id TEXT NULL`
- `managed_comment_status TEXT NULL`
- `linked_issue_count INTEGER NULL`
- `model_execution_mode TEXT NULL`
- `provider_key TEXT NULL`
- `model_key TEXT NULL`
- `effective_config_json TEXT NOT NULL`
- `degraded_features_json TEXT NULL`
- `error_summary TEXT NULL`
- `created_at TEXT/TIMESTAMPTZ NOT NULL`
- `updated_at TEXT/TIMESTAMPTZ NOT NULL`

Indexes and constraints:

- Unique `(repository_id, github_run_id, github_run_attempt)` when both GitHub
  run fields are present.
- Index `(repository_id, created_at)`.
- Index `(pull_request_id, created_at)`.
- Non-negative checks for metric counters and duration.

No run column should store raw diff text, source code, prompts, transcripts, raw
model output, or full review body text.

## Rust Domain and Traits

Add Review Agent domain types in `crates/gateway-core`:

- `ReviewAgentProvider`
- `ReviewAgentRepositoryStatus`
- `ReviewAgentPullRequestState`
- `ReviewAgentRunStatus`
- `ReviewAgentRepositoryRecord`
- `ReviewAgentPullRequestRecord`
- `ReviewAgentRunRecord`
- create/update/upsert input structs for repositories, pull requests, and runs
- `ReviewAgentSettings` for typed feature toggles

Add a `ReviewAgentRepository` trait in
`crates/gateway-core/src/traits.rs` with methods for:

- creating/listing/getting/updating configured repositories
- disabling/reactivating repositories
- looking up a repo by provider identity and by service account binding
- upserting pull request metadata
- starting a run with GitHub run ID/attempt uniqueness
- heartbeat, complete, fail, cancel, skip run state transitions
- listing runs for a configured repository

Compose the trait into `GatewayStore` in
`crates/gateway-store/src/store.rs`.

## Store Implementations

Add backend implementations following existing store conventions:

- `crates/gateway-store/src/libsql_store/review_agent.rs`
- `crates/gateway-store/src/postgres_store/review_agent.rs`
- shared decoding helpers only where existing patterns justify them

Keep SQL explicit and typed. Avoid JSON-only storage for fields that the API
must filter, validate, or render frequently.

## Tests

Add shared store exercises under `crates/gateway-store/src/lib.rs`:

- migrations create all three tables in libSQL and Postgres
- repository create/list/get/update
- default toggle values match #192 decisions
- duplicate active `(provider, owner, name)` is rejected
- duplicate `(provider, external_repository_id)` is rejected when external ID is present
- repositories must reference an existing service account
- pull request upsert preserves `(repository_id, pr_number)` identity
- runs are append-only for distinct GitHub attempts
- duplicate `(repository_id, github_run_id, github_run_attempt)` is rejected or idempotently resolved according to the chosen service behavior
- heartbeat and terminal state updates persist timestamps and metrics
- sanitized metrics and effective config snapshots persist
- check constraints reject invalid enum values and negative metrics

Also add a regression check that no `review_agent_tokens` table is introduced in
this MVP migration.

## Verification

Run through `mise`:

```sh
eval "$(mise activate zsh)"
cargo test -p gateway-store review_agent
cargo test -p gateway-store migration_registry
cargo clippy --workspace --all-targets -- -D warnings
```

If Postgres is available:

```sh
TEST_POSTGRES_URL=... cargo test -p gateway-store postgres_review_agent
```

Run `mise run lint` before handing off if follow-up source changes touch both
Rust and UI.
