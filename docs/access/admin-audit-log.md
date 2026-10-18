# Admin Audit Log

`See also`: [Admin Control Plane](admin-control-plane.md), [Identity and Access](identity-and-access.md), [Observability and Request Logs](../operations/observability-and-request-logs.md)

The audit log records administrator changes to the control plane: who changed what, when, from where, and what the change was. It is separate from request logs. Request logs explain model and tool traffic; the audit log explains changes to keys, identities, budgets, rate limits, sessions and MCP access.

## Where to find it

- In the admin UI, platform admins choose **Audit Log** under **Observability** (`/admin/observability/audit-log`).
- Through the API, `GET /api/v1/admin/audit-logs` with a platform-admin session.

Each entry shows:

- **Time** in UTC.
- **Change**: the target type and action, for example `MCP server` and `Updated`.
- **Target**: the resource's name and ID when the change was made.
- **Actor**: who made the change, and whether it was a user, an API key or the system. A user entry does not record how they signed in: changes made through a session, an invitation link and an identity provider callback all show as a user.
- **IP address** and user agent of the request.
- **Before** and **After**: redacted summaries of the target's state.
- **Details**: extra context, including the request ID.

## What is audited

| Area | Actions |
|---|---|
| API keys | `api_key.created`, `api_key.updated`, `api_key.scopes_updated`, `api_key.revoked`, `api_key.secret_revealed` |
| Users | `user.created`, `user.updated`, `user.deactivated`, `user.reactivated`, `user.onboarding_reset`, `user.invite_issued`, `user.invite_redeemed`, `user.password_changed`, `user.provisioned`, `user.team_transferred` |
| Teams | `team.created`, `team.updated`, `team.members_added`, `team.member_removed` |
| Service accounts | `service_account.created`, `service_account.updated`, `service_account.disabled` |
| Budgets | `budget.set`, `budget.deactivated`, `budget.increase_created` |
| Sessions | `session.created` (password, OIDC or OAuth sign-in), `session.revoked` (logout) |
| MCP servers | `mcp_server.created`, `mcp_server.updated`, `mcp_server.disabled`, `mcp_server.enabled`, `mcp_server.discovery_refreshed` |
| MCP tool sets | `mcp_toolset.created`, `mcp_toolset.updated`, `mcp_toolset.disabled`, `mcp_toolset.tools_replaced` |
| MCP grants | `mcp_grant.set`, `mcp_grant.revoked` |
| MCP tools | `mcp_tool.tags_updated`, `mcp_tool.annotations_overridden`, `mcp_tool.version_approved`, `mcp_tool.version_rejected` |
| MCP tag rules | `mcp_tag_rule.set`, `mcp_tag_rule.revoked` |
| MCP credentials | `mcp_credential_binding.set` (create, replace, or a user's OAuth connect), `mcp_credential_binding.revoked` |
| Provider credentials | `provider_credential.set`, `provider_credential.deleted` |
| Rate limits | `rate_limit_policy.created`, `rate_limit_policy.updated`, `rate_limit_policy.deleted` |
| Other | `pricing_catalog.refreshed`, `review_agent_repository.created`, `.updated`, `.disabled`, `.reactivated` |

A change that fails writes no entry. That includes validation errors, conflicts and denied permissions. Repeating a change that has already happened, such as revoking a revoked key, adding a team's existing members, disabling a disabled repository or MCP server, removing a member another request already removed, signing out twice with the same session, or saving a user, team, budget, API key's access, MCP tool set, MCP tool's tags or annotation overrides, review agent repository, rate-limit policy or service account unchanged, writes no entry either. Some identity changes are made of several writes; if one fails after another has landed, the change is recorded with `outcome: partial` and the error code in **Details**, because part of it took effect.

An invited user's first OIDC or OAuth sign-in records `user.invite_redeemed`, as setting a password from an invite does.

Not recorded:

- Failed sign-in attempts and session refreshes.
- Model traffic, MCP tool calls, batches and Skills publishing. Request logs and MCP invocations cover traffic.
- Guardrail policy, because it is set in `gateway.yaml` and has no admin edit endpoint.
- Deleting an MCP server, because the API only disables and re-enables servers.

## Redaction

Audit entries never store secret values:

- **Secrets and credentials.** Raw API keys, passwords, session cookies, invite links and tokens, MCP upstream secrets and OAuth tokens, and provider tokens are never stored.
- **Credential bindings.** Bindings record metadata only: kind, header name, secret reference, scopes and expiry.
- **Server URLs.** MCP server URLs are stored without credentials, query strings or token-like path segments, including percent-encoded ones.
- **Safety net.** Before an entry is stored, values under secret-looking field names and strings that look like credentials, including URLs with a username or password, are replaced with `[REDACTED]`. Known provider token formats, the same ones the prompt guardrails detect, are replaced with `[REDACTED:<rule>]`.

## Filtering and paging

The list is newest first. Filter by target type, exact action, target ID, actor user ID, or time range (`occurred_at_start` and `occurred_at_end`, RFC 3339). In the UI, choose an actor's name in a row to filter by that actor.

The list leaves out each entry's before, after and details summaries; `GET /api/v1/admin/audit-logs/{id}` returns one entry with them.

Pages use a cursor rather than page numbers. The response's `next_cursor` fetches older entries and is absent on the last page. `limit` defaults to 50 and is capped at 200.

```bash
curl -s -b "ogw_session=$SESSION" \
  "$GATEWAY/api/v1/admin/audit-logs?target_type=api_key&limit=100"
```

## Request IP

The gateway records the address that connected to it. If that address is listed in [`server.trusted_proxies`](../configuration/rate-limits.md#client-ip-and-trusted-proxies), it reads `X-Forwarded-For` instead, as it does for rate limits, so behind a load balancer entries record the browser rather than the balancer. For admin UI changes, the gateway's admin proxy resolves the address this way and signs it, so a client cannot forge it with `X-Forwarded-For` or by sending its own signed header.

## Retention

Entries are kept until you delete them. Request-log retention does not remove audit entries.
