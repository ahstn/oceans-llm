# ADR: Admin Audit Log

- Date: 2026-10-15
- Status: Accepted
- Issue: [#99](https://github.com/ahstn/oceans-llm/issues/99), part of [#458](https://github.com/ahstn/oceans-llm/issues/458)

## Current state

- [Admin Audit Log](../access/admin-audit-log.md) describes what is audited and how to read it.
- [Data Relationships](../contributing/reference/data-relationships.md) lists the `audit_logs` columns.

## Context

The baseline schema had an `audit_logs` table, but only tests wrote to it. Admins could not reconstruct who created or revoked an API key, changed a user's role, moved a team member, edited a budget, or changed MCP server access. Request logs explain model and tool traffic. They do not explain administrator changes.

Several planned features need a durable record of control-plane changes and will write to it: versioned access policies (#447), tool definition approval (#444), delegated MCP server ownership (#445), API key expiry and rotation (#449), and IP allow and deny lists (#450). If there is no shared boundary, each of them would invent its own audit behaviour.

## Decision

### One write boundary

Every audit entry goes through `gateway_service::AdminAudit` (`crates/gateway-service/src/admin_audit.rs`). Handlers do not insert rows themselves.

- `AdminAudit::audited(context, mutation, describe)` awaits the mutation future. Only if it succeeds does it call `describe` with the output and write the entry. A failed mutation never reaches `describe`, so it writes nothing.
- `AdminAudit::audited_steps(context, progress, mutation, describe, partial)` is for a mutation made of several store writes that are not in one transaction, such as creating a user or updating a team. The handler calls `progress.landed()` after each write. On success it behaves like `audited`. If the mutation fails after a write landed, the change is partly applied, so `partial` describes it, the entry carries `outcome: partial` and the failure's error code, and the request still returns the error. A failure before any write records nothing. A handler marks progress only for a write that changed something, and `describe` returns `None` for a success that changed nothing, such as revoking a revoked key.
- `AdminAudit::record(context, event)` writes an entry for a change that has already succeeded, when the success point comes after response shaping.
- `AuditContext` carries the actor and the request IP, user agent and request ID. In HTTP handlers it comes from the `AuditRequestMeta` extractor and the signed-in `UserRecord`: `meta.user_context(&actor)`.
- `AuditEvent` is a small builder: `AuditEvent::new(actions::API_KEY_CREATED, targets::API_KEY).target(id).label(name).before(..).after(..).meta(key, value)`.
- Action and target names are constants in `admin_audit::actions` and `admin_audit::targets`, named `<target_type>.<past-tense verb>`. New code adds a constant rather than a free-form string, so filters and the admin UI see a closed vocabulary.

`AppState::audit()` returns the boundary for the shared store, so a new handler needs the extractor argument, the actor, and one call.

### Write timing and atomicity

The entry is written after the mutation succeeds, as a separate statement. It is not in the mutation's database transaction.

The store has no unit-of-work abstraction. Each repository method opens its own transaction, or none, for each backend. The libSQL store also shares one connection, so a transaction opened around existing methods would include unrelated concurrent requests. Getting real atomicity would mean changing the signature of about sixty store methods in both backends to accept the audit row. That change would conflict with every parallel branch that touches those methods.

A failed audit insert is logged at `error` with the action, target and actor, and the request still succeeds. The mutation has already committed. Returning an error would make the client retry a change that already happened, and for API key creation it would lose the one-time secret shown in the response.

### Record schema

Migration `V61__admin_audit_log.sql` extends the baseline table on both backends. Existing rows become `system` entries, except rows with an `actor_api_key_id`, which become `api_key` entries:

| Column | Meaning |
|---|---|
| `id` | Monotonic integer, part of the cursor. |
| `ts` | Unix milliseconds. The boundary truncates to milliseconds so the cursor round-trips. |
| `actor_kind` | `user`, `api_key` or `system`. |
| `actor_user_id`, `actor_api_key_id` | The acting user, and the API key when one authenticated the change. |
| `actor_label` | The actor's email, captured when the entry is written. |
| `action` | For example `mcp_grant.revoked`. |
| `object_type`, `object_id`, `target_label` | Target type, ID and display name, captured when the entry is written. |
| `before_json`, `after_json` | Redacted summaries of the target's state before and after the change. |
| `details_json` | Redacted metadata, including `request_id`. |
| `request_ip`, `user_agent` | Where the request came from. |

Labels are snapshots, so entries stay readable after the user, key or resource is gone. `actor_user_id` has no foreign key for the same reason. Indexes cover the newest-first list and each filter as `(column, ts, id)`: actor, target type and ID, target ID alone, action, and actor plus action. An action's prefix is its target type, so action plus target type is served by the action index. The list query includes only the filters a request uses and pages with a `(ts, id)` row-value cursor, so a filtered page reads the matching index in order rather than scanning the table.

### Request IP

The gateway serves with `ConnectInfo`. Client addresses are resolved by `admin_ui::client_ip::resolve_client_ip`, configured by `server.trusted_proxies` and shared with request policy. `X-Forwarded-For` is read only when the peer is a configured trusted proxy, so a client that connects directly cannot pick its own address.

The browser always reaches the admin UI through the gateway's own admin proxy, the edge its connection reaches:

- The proxy resolves the client through the trusted proxies, drops any inbound `x-oceans-client-ip` header, and sets its own: `<ip>;<unix seconds>;<HMAC-SHA256>`, keyed by the gateway's identity token secret.
- The admin UI server forwards that header, and the browser's `User-Agent`, on its gateway API calls.
- `resolve_audit_client_ip` accepts the header only when its signature verifies and it is less than five minutes old. Otherwise it records the client resolved through the trusted proxies.
- When `GATEWAY_IDENTITY_TOKEN_SECRET` is unset, its development fallback is public, so the key is a random per-process value instead. Signed addresses then only verify on the replica that signed them, and other replicas record the direct peer.

Behind a load balancer, list it in `server.trusted_proxies` and both admin UI changes and direct API calls record the browser. Without that, they record the balancer's address. The resolver lives in the admin UI crate because the gateway depends on it, so both the proxy and the gateway can share one implementation.

### Redaction

Summaries are built field by field from response views or domain records that hold no secrets. Raw request bodies are never passed in. Before an entry is stored, every summary and metadata value also goes through `redact_audit_value`:

- It redacts a value whose field name matches a request-log secret key, or contains `secret`, `password`, `token`, `authorization`, `cookie`, `private_key`, `api_key`, `ciphertext`, `nonce`, `key_material` or `invite_url`.
- Names ending in `_id`, `_at`, `_kind`, `_mode`, `_type`, `_status`, `_count`, `_prefix`, `_ref` and similar suffixes describe a secret without holding it, so they are kept. Booleans and nulls are kept too.
- It redacts any string that looks like a credential, whatever its field name: a full `gwk_<id>.<secret>` key anywhere in the text, including after punctuation such as `token=` or `(`, an invite link path, a `Bearer` or `Basic` scheme followed by a value of 8+ characters or one that looks encoded (a digit or an inner capital), a URL with userinfo such as `https://user:pass@host`, or a secret-looking name given a value, quoted or not, such as `password=…`, `api_key = '…'`, `?api_key=…` or `Authorization: …`. Short lowercase words after a scheme stay, so phrases such as "bearer token" read normally. Text with percent-encoding is checked again once decoded, so `?api%5Fkey=…` is caught. A tag key shaped like a token is redacted together with its value. The bare `gwk_<id>` display prefix is kept.
- User agents get the same credential check.
- Target and actor labels are user-entered names and emails, so they get the same credential check and are capped at 256 characters.
- Key/value pairs such as entity tags are judged by their `key`, so a `github_pat` tag's value is redacted. Name segments `pat`, `pwd` and `passphrase` also count as secret.
- Strings starting with common provider token prefixes (`ghp_`, `github_pat_`, `glpat-`, `xoxb-`, `sk-ant-`, `AKIA` and similar) are redacted whatever their field name.
- Each summary is capped at 16 KiB. A larger one is replaced with `{ "truncated": true, "bytes": n }`. Oversized metadata keeps its `outcome`, `error` and `request_id`, so a truncated partial entry still reads as partial.

The rules for specific actions are:

- API key creation and secret reveal record no key material.
- Invites record only their expiry, never the URL that carries the token.
- MCP credential bindings record their metadata view: kind, header name, secret reference, scopes and expiry. A secret reference written as a URL loses its credentials and query.
- MCP server and OAuth URLs are stored without credentials, query or fragment, and path segments that look like tokens (a known token shape, or a value of 32+ characters that mixes letters and digits or is one unbroken run of letters or of digits, and is not a hyphenated UUID) become `REDACTED`. Segments are percent-decoded before the check, and kept in their encoded form when they are not tokens.
- Provider credentials record only the user and provider key.
- Review Agent repositories record only the key names of their free-form `settings_json`, never its values.
- Password changes and sign-ins record that they happened and how the user signed in.

### Read access

`GET /api/v1/admin/audit-logs` is platform-admin only. It filters by `actor_user_id`, `action`, `target_type`, `target_id` and an RFC 3339 `occurred_at_start` and `occurred_at_end`. It pages newest first with an opaque `<millis>_<id>` keyset cursor and a `limit` of up to 200. The admin UI shows it at `/admin/observability/audit-log`. List items leave out the before, after and metadata summaries, which can each reach 16 KiB; `GET /api/v1/admin/audit-logs/{id}` returns one entry with them, and the UI loads it when the detail sheet opens.

### Retention

Audit entries are kept indefinitely. Request-log retention and `purge_request_logs_older_than` do not touch `audit_logs`. Maintainers who must prune can delete by `ts` in SQL. A configurable audit retention window is follow-up work.

## Coverage

Audited:

- API keys: create, model-access update, revoke and secret reveal.
- Users: create, update, deactivate, reactivate, onboarding reset, invite issue and redemption, password change, and SSO provisioning.
- Teams: create, update, add members and remove a member. Moving a user between teams is `user.team_transferred`, because its target is the user.
- Service accounts: create, update and disable.
- Budgets: set, deactivate and temporary increase.
- Sessions: password, OIDC and OAuth sign-in, and logout. An invited user's first OIDC or OAuth sign-in also records `user.invite_redeemed`.
- MCP: server create, update, disable and discovery refresh. Tool set create, update, disable and tool replacement. Grant set and revoke. Credential binding create, replace and revoke, including a user's own OAuth connect and disconnect.
- Provider credentials, set by an admin or by the user's own API key.
- Pricing catalog refresh, and Review Agent repository create, update, disable and reactivate.

Not audited:

- **No endpoint yet.** MCP server enable and delete, and guardrail policy edits. Guardrails are configuration only, and the server API only disables. When those endpoints arrive, they use the same boundary.
- **Not a control-plane change.** Failed sign-ins, session touch, OAuth state rows, Skills publishing, batches and Review Agent run callbacks.

## Trade-offs

- An entry can be missing for a change that committed, if the audit insert fails. The failure is logged at `error` with enough context to rebuild the entry, but the log is not the audit trail.
- Multi-step identity handlers (user create, update, deactivate and onboarding reset; team create, update and add members; password change and invite redemption) record a failure partway as `outcome: partial` with a `requested` summary where one exists, not the exact final state. Other single-write handlers have nothing partial to record. Password invite redemption claims the invitation before it writes the password, so two requests racing with one token cannot both set a password; if the password write then fails, the invitation is used up, the entry is partial, and an admin re-issues the invite.
- Old entries are only as complete as their summaries. Fields added to a resource later do not appear in older entries.

## Follow-up

- Move audit writes into the mutation's transaction once the store has a unit of work.
- Add a configurable retention window and an export for SIEM ingestion (see #452).
- Audit failed sign-in attempts as security events, separate from control-plane changes.
