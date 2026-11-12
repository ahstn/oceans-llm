# ADR Self-host secrets with fnox

- Date: 2026-11-11
- Status: Accepted

## Context

The self-host profile stored gateway, provider, administrator, and encryption credentials in the gitignored `mise.selfhost.local.toml`. Git kept the values out of history, but source snapshots, support archives, backups, and every process loading the mise environment could still read the plaintext.

The repository already uses fnox for task-scoped local provider credentials. Self-host daemons also need unattended secret resolution after boot, where an interactive password-manager prompt is unavailable.

## Decision

- Each host defines the self-host values as provider references and an age-encrypted cache in gitignored `fnox.local.toml`, starting from `fnox.local.toml.example`; plaintext values do not belong anywhere in the checkout.
- Configuration validation and both complete gateway daemon definitions clear inherited credential variables, then invoke `fnox exec --only` for the values named by `deploy/selfhost/gateway.yaml` and the gateway runtime.
- The shell, unrelated tasks, and the admin UI daemon receive none of those values.
- `mise.selfhost.local.toml` remains available for non-secret machine-specific paths and settings.
- A host migrating from plaintext must encrypt or move every value first, verify decryption, delete the plaintext file, and rotate any credential that may have left the machine in a snapshot or backup.

## Why

This closes the snapshot disclosure path without making daemon startup depend on an interactive session. Keeping the grant at validation and gateway startup preserves the existing self-host workflow while reducing ambient credential exposure.

## Trade-offs

- A process running as the same OS user can invoke fnox and use the configured decryption identity. This protects snapshots and accidental inheritance; it is not a same-user sandbox.
- Unattended restarts require a non-interactive age identity on the host. Protect that identity separately and include it in recovery procedures.
- Removing an old plaintext file cannot revoke a credential from an already disclosed snapshot. Rotation remains an operational requirement when disclosure is possible.

## Follow-ups

- Keep support-bundle and backup generation from exporting decryption identities.
- Re-run the encrypted cache sync after source credentials rotate.
