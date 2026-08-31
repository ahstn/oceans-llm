# API keys

API Keys lets an authorized user review keys in scope, create a key for an owner allowed by their role, change model access for user-owned keys, and revoke a key. A newly created key exposes its raw secret in the creation result. An authorized team or platform administrator can reveal an active service-account-owned key later.

## Sub-features

- `keys-list` shows API keys allowed by the current user's scope.
- `keys-create` creates a named key for an owner allowed by the signed-in user's role.
- `keys-secret-once` shows every newly created raw key in the creation result; user-owned keys cannot be revealed again.
- `keys-manage` opens effective model access and, for an authorized active service-account key, later reveal controls. Service-account model grants are fixed by configuration.
- `keys-revoke` revokes a key and prevents later gateway use.

## How to get to it (user POV)

- Sign in and choose `API Keys` under `Control Plane`.
- Open `/admin/api-keys`; an unauthenticated user is sent to sign-in first.
- Use `Create API key` for a new key or `Manage` in a key row for an existing key.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- Sign in as a user whose action permissions include the operation under test.
- Use a unique key name that contains the verification run ID.

- **List.** Choose `API Keys`. The `API keys` heading and the key list appear for the current scope.
- **Open create.** Choose `Create API key`. A creation dialog appears with `Name`, `Owner type`, owner selection, and model access controls.
- **Create.** Fill `Name`, choose an allowed owner, set model access, and choose the final `Create API key` button. The element `new-api-key-raw-key` contains the one-time raw key.
- **Confirm use.** Call `/v1/models` with the new key and confirm that returned model IDs equal the effective intersection of the selected grants and owner/model allowlists. Do not store the raw key in proof.
- **Manage user key.** Locate the row by the unique name and choose `Manage`. The `Manage API key` dialog shows the masked key identity and current owner. It permits model-access changes and does not offer `Reveal API key` for a user-owned key.
- **Service-account reveal availability.** Manage the seeded active `Local CI Runner Key`. The dialog shows fixed explicit model access, `Credential secret`, and `Reveal API key` when the session has `reveal_api_key`. Confirm the control without revealing or recording the secret.
- **Revoke and cleanup.** Choose `Revoke key`, then confirm that `/v1/models` rejects the key. The created record can remain as a revoked audit record; record its name and revoked state.
- **Proof.** Capture the create form, masked created result, managed access state, and revoked list state. Redact the raw key from screenshots, traces, logs, and JSON.

## Gotchas

- This feature mutates `gateway.db`; do not run it as part of the read-only baseline.
- Every newly created raw key is shown in the creation result. User-owned keys are shown only once. Active service-account-owned keys can be revealed later by an authorized team or platform administrator. Never save either secret as an artifact.
- Revoked, user-owned, and unauthorized keys do not show the later reveal control.
- Model access and owner options depend on the signed-in user's permissions.
- Revocation is the cleanup. Do not delete database files to remove one verification record.
