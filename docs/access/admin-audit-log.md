# Admin Audit Log

`See also`: [Admin Control Plane](admin-control-plane.md), [Identity and Access](identity-and-access.md), [Observability and Request Logs](../operations/observability-and-request-logs.md), [ADR: Admin Audit Log](../adr/2026-10-15-admin-audit-log.md)

The audit log records administrator changes to the control plane: who changed what, when, from where, and what the change was. It is separate from request logs. Request logs explain model and tool traffic; the audit log explains changes to keys, identities, budgets, sessions and MCP access.

## Where to find it

- In the admin UI, platform admins choose **Audit Log** under **Observability** (`/admin/observability/audit-log`).
- Through the API, `GET /api/v1/admin/audit-logs` with a platform-admin session.

Each entry shows:

- **Time** in UTC.
- **Change**: the target type and action, for example `MCP server` and `Updated`.
- **Target**: the resource's name and ID when the change was made.
- **Actor**: who made the change, and whether they used a session or an API key.
- **IP address** and user agent of the request.
- **Before** and **After**: redacted summaries of the target's state.
- **Details**: extra context, including the request ID.

## What is audited

| Area | Actions |
|---|---|
| API keys | `api_key.created`, `api_key.updated`, `api_key.revoked`, `api_key.secret_revealed` |
| Users | `user.created`, `user.updated`, `user.deactivated`, `user.reactivated`, `user.onboarding_reset`, `user.invite_issued`, `user.invite_redeemed`, `user.password_changed`, `user.provisioned` |
| Teams | `team.created`, `team.updated`, `team.members_added`, `team.member_removed`, `team.member_transferred` |
| Service accounts | `service_account.created`, `service_account.updated`, `service_account.disabled` |
| Budgets | `budget.set`, `budget.deactivated`, `budget.increase_created` |
| Sessions | `session.created` (password, OIDC or OAuth sign-in), `session.revoked` (logout) |
| MCP servers | `mcp_server.created`, `mcp_server.updated`, `mcp_server.disabled`, `mcp_server.discovery_refreshed` |
| MCP tool sets | `mcp_toolset.created`, `mcp_toolset.updated`, `mcp_toolset.disabled`, `mcp_toolset.tools_replaced` |
| MCP grants | `mcp_grant.set`, `mcp_grant.revoked` |
| MCP credentials | `mcp_credential_binding.set` (create, replace, or a user's OAuth connect), `mcp_credential_binding.revoked` |
| Provider credentials | `provider_credential.set`, `provider_credential.deleted` |
| Other | `pricing_catalog.refreshed`, `review_agent_repository.created`, `.updated`, `.disabled`, `.reactivated` |

A change that fails writes no entry. That includes validation errors, conflicts and denied permissions.

Not recorded:

- Failed sign-in attempts and session refreshes.
- Model traffic, MCP tool calls, batches and Skills publishing. Request logs and MCP invocations cover traffic.
- Guardrail policy, because it is set in `gateway.yaml` and has no admin edit endpoint.
- Enabling or deleting an MCP server, because the API only disables servers today.

## Redaction

Audit entries never store secret values:

- **Secrets and credentials.** Raw API keys, passwords, session cookies, invite links and tokens, MCP upstream secrets and OAuth tokens, and provider tokens are never stored.
- **Credential bindings.** Bindings record metadata only: kind, header name, secret reference, scopes and expiry.
- **Server URLs.** MCP server URLs are stored without credentials or query strings.
- **Safety net.** Before an entry is stored, values under secret-looking field names and strings that look like credentials are replaced with `[REDACTED]`.

## Filtering and paging

The list is newest first. Filter by target type, exact action, target ID, actor user ID, or time range (`occurred_at_start` and `occurred_at_end`, RFC 3339). In the UI, choose an actor's name in a row to filter by that actor.

Pages use a cursor rather than page numbers. The response's `next_cursor` fetches older entries and is absent on the last page. `limit` defaults to 50 and is capped at 200.

```bash
curl -s -b "ogw_session=$SESSION" \
  "$GATEWAY/api/v1/admin/audit-logs?target_type=api_key&limit=100"
```

## Request IP

When the admin UI makes a change, the gateway records the browser's address. When requests reach the gateway through a load balancer or ingress on a private address, the gateway uses the right-most public address in `X-Forwarded-For`. A request from a public address directly records that address.

## Retention

Entries are kept until you delete them. Request-log retention does not remove audit entries.
