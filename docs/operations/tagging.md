# Tagging

`See also`: [Observability and Request Logs](observability-and-request-logs.md), [Request Logs](observability/request-logs.md), [Identity and Access](../access/identity-and-access.md), [Budgets and Spending](budgets-and-spending.md), [Data Relationships](../reference/data-relationships.md), [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md)

Tags are bounded key/value metadata that admins and callers can use to connect Oceans activity to their own systems. They are meant for attribution, filtering, export, and reconciliation, not for authorization or secret storage.

Use tags when the built-in ownership fields are not enough. Oceans already records durable users, teams, service accounts, API keys, providers, models, request ids, and spend ledger rows. Tags add admin-controlled and caller-controlled dimensions such as cost center, application, workload, deployment, environment, or external owner id.

## Tag Surfaces

Oceans has two tag surfaces.

- Request tags are supplied by clients on each data-plane request. They are stored with request logs and support request-log filtering.
- Identity tags are managed by admins on users and teams. They are visible in user and team detail dialogs and are intended for future export, observability, and spend-attribution workflows.

Both surfaces follow the same key/value rules so teams do not need two naming schemes.

## Choosing Tags

Prefer tags that are stable, low-cardinality, and meaningful outside Oceans.

Good examples:

- `cost-center=platform`
- `workload=agent-harness`
- `app=internal-support`
- `external-owner=finops`
- `deployment=prod-eu`

Avoid:

- secrets, tokens, emails, prompts, or other sensitive data
- per-request unique values such as trace ids, timestamps, session ids, or full URLs
- values that change whenever code is deployed unless deployment identity is the point of the tag
- duplicating built-in fields such as user id, team id, model, provider, or request id

Use the smallest vocabulary that answers a real operational question. Every new tag key becomes something admins may need to keep consistent during exports and reporting.

## Validation Rules

Tag rules are intentionally strict.

- At most 5 bespoke request tags or 5 identity tags are accepted.
- Tag keys are capped at 32 characters.
- Tag values are capped at 64 characters.
- Keys and values cannot be empty after trimming.
- Keys must start with a lowercase ASCII letter.
- Keys may contain lowercase ASCII letters, digits, `.`, `_`, and `-`.
- Values must contain lowercase ASCII letters, digits, `.`, `_`, `-`, `/`, or `:`.
- Keys must be unique within the same tag set.
- Reserved keys `service`, `component`, and `env` cannot be used as bespoke tag keys.

The reserved keys are owned by the universal request-tag headers. Use those headers for request service, component, and environment. Use different names such as `app`, `workload`, `tenant`, or `deployment` for bespoke dimensions.

## Request Tags

Clients can attach request tags with these headers:

- `x-oceans-service`
- `x-oceans-component`
- `x-oceans-env`
- `x-oceans-tags`

The universal headers are optional and may only be sent once each.

`x-oceans-tags` is also optional, may only be sent once, and uses semicolon-separated key/value pairs:

```text
x-oceans-tags: cost-center=platform; workload=agent-harness
```

Request tags are captured at the gateway boundary and written to request-log data when request logging persists a row for that request. The request-log list supports filters for `service`, `component`, `env`, and one bespoke `tag_key`/`tag_value` pair.

Request tags describe the caller's view of one request. They do not change identity ownership, budget enforcement, model access, or API-key permissions.

## Identity Tags

Admins can set tags on users and teams from the admin identity UI.

Identity tags describe durable ownership context. They are useful when the organization has external systems that do not map cleanly to Oceans user or team names, such as:

- cost allocation hierarchies
- internal product or workload catalogs
- compliance or data-boundary labels
- external owner ids used by reporting systems

Identity tags are displayed when an admin opens a user or team detail dialog. They do not currently affect runtime authorization, budget checks, request routing, or request-log filtering.

Future exports can combine identity tags with spend or observability data so downstream systems can reconcile Oceans usage with existing organizational metadata.

## Storage and Retention

Request tags and identity tags have different retention behavior.

- Request tags are operational request-log detail. They are removed when request-log retention purges the parent request-log row.
- Identity tags are durable user/team metadata. Request-log retention does not remove users, teams, or their tags.
- Spend ledger rows remain separate from request-log retention. Future export jobs should join or enrich ledger data intentionally instead of assuming request-log detail is always retained.

For table-level relationships, see [Data Relationships](../reference/data-relationships.md).

## What This Page Does Not Own

- Request-log payload capture, redaction, stream parsing, and purge mechanics: [Observability and Request Logs](observability-and-request-logs.md)
- Per-request list/detail behavior: [Request Logs](observability/request-logs.md)
- User, team, service-account, and API-key lifecycle rules: [Identity and Access](../access/identity-and-access.md)
- Spend ledger semantics and budget enforcement: [Budgets and Spending](budgets-and-spending.md)

Validate documentation-only edits with `mise run docs:check`.
