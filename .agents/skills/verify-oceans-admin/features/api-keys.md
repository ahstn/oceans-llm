# API keys

API Keys lets an authorized user review keys in scope, create a key for an owner allowed by their role, change model access for user-owned keys, limit a key to capability scopes and MCP servers, and revoke a key. A newly created key exposes its raw secret in the creation result. An authorized team or platform administrator can reveal an active service-account-owned key later.

## Sub-features

- `keys-list` shows API keys allowed by the current user's scope.
- `keys-create` creates a named key for an owner allowed by the signed-in user's role.
- `keys-secret-once` shows every newly created raw key in the creation result; user-owned keys cannot be revealed again.
- `keys-manage` opens effective model access and, for an authorized active service-account key, later reveal controls. Service-account keys are locked to `Selected models`, but their `Granted models` list stays editable through `Save access`.
- `keys-scopes` limits a key to capability scopes (`inference`, `mcp:aggregate`, `mcp:proxy`) and, with an MCP scope, to selected MCP servers. Set on create or later in `Manage API key`; saved through `PUT /api/v1/admin/api-keys/{api_key_id}/scopes`. See [ADR: scoped API keys](../../../../docs/adr/2026-10-17-scoped-api-keys.md).
- `keys-revoke` revokes a key and prevents later gateway use.

## How to get to it (user POV)

- Sign in and choose `API Keys` under `Control Plane`.
- Open `/admin/api-keys`; an unauthenticated user is sent to sign-in first.
- Use `Create API key` for a new key or `Manage` in a key row for an existing key.
- Open `/admin/api-keys?api_key_id=<id>` to land directly in `Manage API key`.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- Sign in as a user whose action permissions include the operation under test.
- Use a unique key name that contains the verification run ID.

- **List.** Choose `API Keys`. The `API keys` heading and the `Keys` card appear for the current scope.
- **Open create.** Choose `Create API key`. A creation dialog appears with `Name`, `Owner type`, owner selection, and model access controls.
- **Create.** Fill `Name`, choose an allowed owner, set model access, and choose the final `Create API key` button. The toast reads `API key created`, and the `Copy the new key now` alert's `new-api-key-raw-key` element holds a raw key starting with `gwk_`. Use it in memory only, then choose `Dismiss` before any screenshot.
- **Confirm use.** Call `/v1/models` with the new key and confirm that returned model IDs equal the effective intersection of the selected grants and owner/model allowlists. Do not store the raw key in proof.
- **Scopes on create.** In the create dialog, set the `Capabilities` toggle to `Limited` and tick `MCP proxy` (`create-api-key-scope-mcp:proxy`). The `MCP servers` toggle appears; choose `Selected servers` and tick one seeded server. `Create API key` stays disabled until at least one scope, and at least one server under `Selected servers`, is ticked. The new row's `Scopes` column reads `MCP proxy · <server_key>`.
- **Scope refusal.** With that key, `GET /v1/models` returns `403` with `error.code` `insufficient_scope`. `POST /mcp/<listed server>` with `tools/list` passes the scope check. `POST /mcp/<other server>` returns `403` with a JSON-RPC error (`error.code` `-32001`, `error.data.error` `insufficient_scope`).
- **Edit scopes.** In `Manage API key`, set `Capabilities` back to `Full access` and choose `Save access`. The toast reads `API key updated`, the row's `Scopes` column reads `Full access`, `GET /v1/models` with the same key now succeeds, and the audit log shows one `api_key.scopes_updated` entry with `before` and `after`.
- **Manage user key.** Locate the row by the unique name and choose `Manage`. The `Manage API key` dialog shows the masked key identity and current owner. It permits model-access changes and does not offer `Reveal API key` for a user-owned key.
- **Service-account reveal availability.** Manage the seeded active `Local CI Runner Key`. The `Model grant mode` radios are disabled, and `manage-api-key-secret` shows `Credential secret` and `Reveal API key` when the session has `reveal_api_key`. Confirm the control is present, but never click it. Choose `Cancel`.
- **Revoke and cleanup.** In `Manage API key`, under `Lifecycle actions`, choose `Revoke key`. It acts immediately, with no confirmation. The toast reads `API key revoked` and the row Status reads `revoked`. Confirm that `/v1/models` rejects the key. The created record can remain as a revoked audit record; record its name and revoked state.
- **Proof.** Capture the create form, masked created result, managed access state, and revoked list state. Redact the raw key from screenshots, traces, logs, and JSON.

## Gotchas

- The `Owner user` picker lists the seeded demo users but not the bootstrap `admin@local`. Use a seeded user such as `alice@platform.local` as the owner of a temporary key.
- This feature mutates `gateway.db`; do not run it as part of the read-only baseline.
- Every newly created raw key is shown in the creation result. User-owned keys are shown only once. Active service-account-owned keys can be revealed later by an authorized team or platform administrator. Never save either secret as an artifact.
- Revoked, user-owned, and unauthorized keys do not show the later reveal control. Revoked keys show the `Revoked keys are read-only` alert.
- The `Copy the new key now` alert says the secret cannot be revealed again, even for service-account keys that can be. For those keys, trust `Reveal API key` in Manage, not the alert copy.
- The seeded `Local CI Runner Key` stores only a hash. Revealing it fails with `api key secret is not retrievable for this key`, so check only that the control is present.
- Model access and owner options depend on the signed-in user's permissions.
- Keys without scopes, including every seeded key, show `Full access` in the `Scopes` column.
- The `MCP servers` field only appears when `Full access`, `MCP aggregate`, or `MCP proxy` is selected. Unticking the last MCP scope clears the server list.
- A scope edit and a model-access edit in one save are two requests: `PATCH` for model access, then `PUT .../scopes`. Each is audited separately.
- Revocation is the cleanup. Do not delete database files to remove one verification record.
