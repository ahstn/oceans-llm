# Audit Log

Audit Log is a read-only, platform-admin-only view of administrator changes to the control plane. Each entry records the actor, the action, the target, redacted before and after summaries, and the request IP and user agent. Entries come from API key, identity, team, service account, budget, sign-in, MCP server, tool set, grant, credential binding, provider credential, pricing catalog, and Review Agent repository mutations. A failed mutation writes no entry, and secrets, tokens, passwords and invite links are never stored.

## Sub-features

- `audit-log-list` lists entries newest first with UTC time, a `Change` column (target type plus an action badge such as `Created` or `Revoked`), `Target` label and ID, `Actor` with `Session` or `API key`, and `IP address`.
- `audit-log-filter` narrows entries through URL search: the `Target type` select, and `Action`, `Target ID` and `Actor user ID` search fields that apply on Enter. Choosing an actor name in a row filters by that actor.
- `audit-log-page` pages with an opaque cursor: `Older` appears while more entries exist, and `Newest` returns to the first page.
- `audit-log-detail` opens a sheet when a row is chosen. It shows the action, target, actor, IP address, user agent, and `Before`, `After` and `Details` JSON blocks.

## How to get to it (user POV)

- Sign in as a platform administrator and choose `Audit Log` under `Observability`.
- Open `/admin/observability/audit-log` directly. Filters are URL search params, for example `?target_type=api_key&action=api_key.created`.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- You are signed in as the seeded `admin@local`. The sidebar entry needs the `request_logs` page and the `platform_admin` role, and `GET /api/v1/admin/audit-logs` rejects non-platform-admins.
- The demo seed writes no audit entries, but signing in writes a `session.created` entry, so a fresh run starts with at least one row.

Steps:

- **Open.** Choose the `Audit Log` link. The `Audit Log` heading and the `Control-plane changes` frame are visible, and the newest row is the sign-in (`Session`, `Created`) attributed to `admin@local` with `127.0.0.1` as the IP address.
- **One change per area.** Through the UI, create a temporary user API key on `API Keys` and revoke it, create a team on `Teams`, set a budget for a user on `Spend Controls`, and create a tool set on `MCP Servers` → `Tool Sets`. Return to `Audit Log`. Rows for `API key Created`, `API key Revoked`, `Team Created`, `Budget Set` and `MCP toolset Created` appear above the sign-in, each with `admin@local` as the actor.
- **No secret.** Open the `API key Created` row. The `After` block shows the key name, prefix, owner and status, and no raw `gwk_` key. The revealed key from the create dialog appears nowhere on the page.
- **Failed change.** Try to create a team with a name that is only spaces. The request fails with a validation error and no new `Team` row appears.
- **Filter.** Choose `API key` in `Target type`. Only API key rows remain and the URL search holds `target_type=api_key`. Choose `Clear`.
- **API parity.** With the same session, fetch `GET /api/v1/admin/audit-logs?limit=200`. The `id`, `action`, `target_id` and `actor.user_id` of the rendered rows must match `data.items` in order, and the response must not contain the raw API key.
- **Proof.** Capture the list after the changes, the filtered list, and the opened `API key Created` sheet. Record the entry IDs.

## Gotchas

- Entries are never purged by request-log retention, so a reused `gateway.db` keeps entries from earlier runs. Compare against the rows newer than the run's sign-in entry.
- `Older` uses a keyset cursor, not page numbers; the browser's back button returns to the previous page.
- The admin UI calls the gateway API from its own server, so the IP address comes from the `X-Forwarded-For` chain the gateway's admin proxy adds. Locally that is `127.0.0.1`.
- Guardrail policies are configuration only, so there are no guardrail policy entries yet.
