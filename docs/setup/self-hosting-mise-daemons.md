# Self-Hosting with mise Daemons

`See also`: [Deploy and Operations](deploy-and-operations.md), [Runtime Bootstrap and Access](runtime-bootstrap-and-access.md), [Admin Runbooks](../operations/operator-runbooks.md), [ADR: Self-hosting with mise daemons](../adr/2026-10-01-self-hosting-with-mise-daemons.md)

This page explains how to run Oceans LLM on one Linux host, update it by hand, and reach it only over Tailscale. No CI system touches the host. The examples use a host named `hogwarts`. Any host works.

## Runtime Shape

```text
tailnet client --HTTPS--> tailscaled (serve :443)
                              |
                              v
                     gateway   127.0.0.1:8080  --/admin*--> admin-ui 127.0.0.1:3001
                              |
                              v
                     db (Postgres 18) 127.0.0.1:5432
```

- `mise.selfhost.toml` declares the `gateway` and `admin-ui` daemons. It also redeclares `db` for a fixed port and boot start. The file loads only in the `selfhost` config environment, so laptops and CI never see it.
- The `db` daemon is also declared in the root `mise.toml`. Developers use it for local Postgres (`mise run //crates/gateway-store:postgres:start`). It replaced the old `pitchfork.toml` and `scripts/postgres-pitchfork.sh`.
- Pitchfork supervises the daemons. Mise generates the Pitchfork config.
- Every listener binds to `127.0.0.1`. `tailscale serve` is the only way in.
- The gateway and UI run from source built on the host. Nothing is pulled from a registry.

## Optional Skills Storage with RustFS

Skills use an S3 bucket for their archives. An external S3 service needs no additional daemon. For a local S3 service, the optional RustFS profiles use the upstream native binary with the same mise and Pitchfork lifecycle as Postgres. The profiles pin RustFS 1.0.1 and AWS CLI 2.27.49. They require mise 2026.9.18 or later and a current Pitchfork installation.

RustFS provides binaries for Linux x86-64 and ARM64, and macOS Apple Silicon. Current releases do not provide Intel macOS binaries; use a source build or a container there. See the [upstream installation guide](https://docs.rustfs.com/en/installation/macos). A single local disk is suitable for development and a small single-host deployment, but it provides no disk redundancy.

### Local development

From the repository root, run:

```sh
mise -E rustfs run rustfs:setup
mise -E rustfs run rustfs:status
mise -E rustfs run rustfs:test
```

To run the focused Skills UI and CLI tests against a real gateway and this object store, use `mise -E rustfs run rustfs:e2e`. The task builds both binaries and installs the browser dependencies. It uses gateway, UI, and mock-provider ports `49480`, `49481`, and `49482` by default; the existing `E2E_*_PORT` variables can override them. Each run writes objects under its own `skills-e2e/<runtime>/` prefix. Stack cleanup removes only that prefix and retains the bucket and other skills.

The setup task creates random credentials in `.local/rustfs/credentials.env` with mode `600`, starts RustFS, waits for `/health/ready`, and creates the `oceans-skills` bucket if it is absent. Repeating the task retains the existing credentials and bucket. Other bucket errors, including denied access, cause the task to fail.

The development daemon stores data in `.local/rustfs/data`, which is ignored by Git. Each checkout has its own directory. The primary checkout uses port `9000`; linked worktrees use a derived port. Mise does not search for a free port. Inspect the endpoint without printing credentials:

```sh
mise -E rustfs x -- sh -c 'printf "%s\n" "$OCEANS_SKILLS_S3_ENDPOINT"'
```

RustFS binds only to `127.0.0.1`. The console is disabled through `RUSTFS_CONSOLE_ENABLE=false`. For the pinned release, `--console-enable` is a switch: a following `false` is parsed as a volume path. Do not pass `--console-enable false`.

Use these commands to manage the service:

```sh
mise -E rustfs run rustfs:logs
mise -E rustfs daemons restart rustfs
mise -E rustfs run rustfs:stop
```

Stop and restart retain data. No reset task removes the bucket or data directory. A missing credentials file with an existing data directory is an error; restore the file from backup. Avoid `mise env` or `rustfs server --help` in shared logs because both can display credentials from the environment.

Use `OCEANS_SKILLS_S3_ENDPOINT` for the Skills storage endpoint, `us-east-1` for the region, and `oceans-skills` for the bucket. Enable path-style addressing and HTTP for this loopback endpoint. The generated file supplies `RUSTFS_ACCESS_KEY` and `RUSTFS_SECRET_KEY`. See [Skills setup](skills.md) for the gateway configuration and client workflow. Run the gateway or its test command under `mise -E rustfs x -- ...` to load these values.

### Self-hosted RustFS

Complete the base host setup and secrets configuration below first. Then select all three profiles in this order:

```toml
# .miserc.local.toml
env = ["selfhost", "rustfs", "selfhost-rustfs"]
```

Run `mise run rustfs:setup` before the first deployment. Add the Skills configuration described above to your gateway YAML. The base `selfhost` profile remains suitable for external S3 and does not start RustFS.

The final profile replaces the full RustFS daemon definition. It uses fixed port `9000`, sets `boot_start = true`, and stores data at `$HOME/.local/share/oceans-llm/rustfs/data`, outside the checkout. The generated credentials remain in `.local/rustfs/credentials.env`; back up that file separately. A one-shot bucket check runs after RustFS is ready and before the gateway starts. The gateway still waits for Postgres. Existing Pitchfork boot registration starts this dependency chain after a host restart.

To choose another durable data location, set `OCEANS_RUSTFS_DATA_DIR` in `mise.selfhost-rustfs.local.toml`. Stop RustFS before moving existing data, copy it with its permissions, and retain the original until a download check passes. For deployment, use a bucket-scoped gateway identity if other applications also use this RustFS instance. Keep the root credentials for storage administration.

### Backup and restore

`selfhost-backup` only creates a PostgreSQL dump. Skills also require their S3 objects. A database dump alone cannot restore uploaded archives.

For a consistent backup on this single host, stop the gateway and UI so uploads and deletions cannot change metadata, then stop RustFS. Capture the PostgreSQL dump, the complete RustFS data directory, and the credentials file as one backup set. Protect and copy that set off the host. Start RustFS and then the gateway and UI after the backup completes. Do not copy live RustFS data files as a substitute for a tested storage backup procedure.

Restore metadata and objects from the same backup set. Start the service, download an existing skill version, and compare its SHA-256 digest with the stored version digest before reopening access. For external S3, use the storage provider's backup or replication controls and retain the same metadata-to-object consistency requirement.

## One-Time Setup

### 1. Host prerequisites

- Linux with `systemd`, `curl`, `git`, and `mise` installed.
- Build dependencies. The `postgres` tool compiles PostgreSQL from source on Linux, and the gateway needs a C toolchain. On Debian or Ubuntu:

  ```sh
  sudo apt install build-essential pkg-config bison flex libreadline-dev uuid-dev libicu-dev zlib1g-dev libssl-dev
  ```

  Without `bison`, `flex`, `libreadline-dev`, or `uuid-dev`, `mise install` fails with `Failed to configure PostgreSQL`.
- Tailscale running and logged in. In the Tailscale admin console, enable MagicDNS and HTTPS certificates.
- A normal user, not root. The Postgres preset refuses to run as root.

### 2. Clone and select the environment

```sh
git clone https://github.com/ahstn/oceans-llm.git ~/oceans-llm
cd ~/oceans-llm
printf 'env = ["selfhost"]\n' > .miserc.local.toml
mise trust
mise install
```

`.miserc.local.toml` selects the `selfhost` config environment for this checkout. It is git-ignored. Plain `mise` commands, `ssh hogwarts 'cd ~/oceans-llm && mise run deploy'`, and the supervisor all pick it up without a shell profile. `MISE_ENV` and `-E` still override it. Run `mise config` to confirm that `mise.selfhost.toml` is loaded.

### 3. Create the secrets file

Secrets stay out of git in `mise.selfhost.local.toml`. Mise loads `mise.{env}.local.toml` after `mise.{env}.toml`, and the git-ignore rules `mise.*.local.toml` cover it. The base overlay declares each variable `required`, so a missing value stops mise before any daemon starts.

```sh
umask 077
cat > mise.selfhost.local.toml <<EOF
[env]
OCEANS_API_KEY_SECRET_ENCRYPTION_KEY = "$(openssl rand -base64 32)"
GATEWAY_BOOTSTRAP_ADMIN_PASSWORD = "$(openssl rand -base64 18)"
GATEWAY_API_KEY = "gwk_hogwarts.$(openssl rand -hex 24)"
GATEWAY_PUBLIC_BASE_URL = "https://hogwarts.<tailnet>.ts.net"
OPENROUTER_API_KEY = "<key>"
EOF
```

Replace `<tailnet>` and `<key>` (create the key at <https://openrouter.ai/settings/keys>). Back up this file somewhere safe. If you lose `OCEANS_API_KEY_SECRET_ENCRYPTION_KEY`, stored managed API keys cannot be decrypted.

The overlay lists these names under `redactions`, so mise masks them in captured task output. Redaction does not encrypt the file. `mise env` still prints plain values.

Other ways to supply secrets, if a plain file is not enough:

| Option | Notes |
| --- | --- |
| `[env] _.file = "/path/secrets.env"` | A dotenv, JSON, YAML, or TOML file. Accepts an array of files and `redact = true`. Use it for a file outside the checkout. |
| [sops](https://mise.jdx.dev/environments/secrets/sops.html) (experimental) | Commit an encrypted file. Load it with `_.file`. Needs a decryption identity on the host. |
| [age values](https://mise.jdx.dev/environments/secrets/age.html) (experimental) | Encrypted values inside `mise.selfhost.toml`. Needs an age or SSH identity. |
| [fnox](https://mise.jdx.dev/environments/secrets/) | Mise's recommended manager. Run `fnox exec -- mise run deploy`. |

### 4. Edit the gateway config

`deploy/selfhost/gateway.yaml` holds one OpenRouter provider and three models as a starting point: `gpt-sol-latest`, `claude-sonnet-latest`, and `deepseek-flash-latest`. They map to OpenRouter's `~provider/model-latest` aliases. Keep the quotes around `~...` in YAML. Edit providers, models, and teams for your use. Add any new `env.*` secret as `{ required = true }` in `mise.selfhost.toml`, and its value in `mise.selfhost.local.toml`. Add the name to `redactions`.

### 5. Allow your user to manage Tailscale

```sh
sudo tailscale set --operator="$USER"
```

`deploy` publishes the gateway with `tailscale serve`. A non-root user needs this operator setting first, or the last step of the first deploy fails.

### 6. First deploy

```sh
mise run deploy
```

This builds the UI and the release gateway, starts Postgres, and starts the gateway and UI. Migrations, config seeding, and bootstrap admin run on gateway start. The first Rust build is slow.

### 7. Expose on the tailnet

`deploy` already runs `selfhost-expose`. To repeat it alone:

```sh
mise run selfhost-expose
```

`selfhost-expose` runs `tailscale serve --bg --https=443 http://127.0.0.1:8080` and is safe to repeat. `deploy` runs it as its last step. Serve config lives inside `tailscaled` and persists across reboots, so no daemon supervises it. The first run may print a `login.tailscale.com` link: open it to enable Serve and HTTPS certificates for the node, then run the task again.

Serve is not strictly required. Binding the gateway to the Tailscale IP would also work, but the session cookie then loses its `Secure` flag (the gateway sets it only when it sees `x-forwarded-proto: https`), and the port is exposed on that interface. Serve avoids both.

Do not run `tailscale funnel`. Funnel makes the service public. Limit who can reach `hogwarts:443` with Tailscale ACLs.

Sign in at `https://hogwarts.<tailnet>.ts.net/admin` as `admin@local` with `GATEWAY_BOOTSTRAP_ADMIN_PASSWORD`. The gateway forces a password change.

### 8. Start at boot

```sh
pitchfork boot enable
sudo loginctl enable-linger "$USER"
pitchfork boot status
```

`boot enable` registers a user `systemd` service. `enable-linger` lets that service start at boot without a login. The three daemons carry `boot_start = true`, so the supervisor starts them in dependency order.

Optional: stop CLI commands from starting a second, unmanaged supervisor.

```toml
# ~/.config/pitchfork/config.toml
[settings.supervisor]
auto_start = false
```

## Update

Updates are manual. Run on `hogwarts` (directly or over `ssh hogwarts` / `tailscale ssh hogwarts`):

```sh
cd ~/oceans-llm
mise run deploy              # latest v* tag on origin
mise run deploy v0.35.0      # a specific tag or commit
```

`deploy` does the following:

1. Refuses to run if tracked files have local changes.
2. Fetches tags and checks out the ref in detached mode.
3. Starts a new `mise` process, so the new revision's config and tasks apply.
4. Builds the UI and the release gateway.
5. Dumps the database to `.local/backups/` (mode 600, kept 30 days). It starts `db` first if Postgres is stopped, so the dump always runs.
6. Runs `gateway config validate`.
7. Makes sure `db` runs, and restarts `gateway` and `admin-ui`.
8. Checks `/readyz` and `/admin/login` on the loopback listener.
9. Publishes the gateway on the tailnet (`selfhost-expose`).

The gateway is down for the restart window. Postgres is not restarted.

## Rollback

```sh
mise run deploy v0.34.0
```

Rules:

- The target tag must contain `mise.selfhost.toml`.
- Migrations are not reversed. If the older version cannot read the newer schema, stop the app daemons and restore a dump:

```sh
mise daemons stop gateway admin-ui
pg_restore --clean --if-exists -d "$POSTGRES_URL" .local/backups/oceans-<timestamp>.dump
mise run deploy v0.34.0
```

## Operate

```sh
mise daemons ls
mise daemons logs gateway
mise daemons restart gateway
mise daemons tui
```

## Security Notes

- No inbound CI path exists. The host pulls from `origin` only when you run `deploy`.
- Deploying a tag pins the revision. The task does not verify signatures. If you start signing tags, add `git verify-tag "$ref"` to `deploy`.
- `mise.selfhost.local.toml` holds plain secrets. Keep it mode 600 and out of backups that leave the host unencrypted.
- Postgres uses the `mise` preset: `trust` authentication on `127.0.0.1`. Any local user on the host can connect. This is acceptable for a single-user host. Use a custom daemon with a password if that changes.
- Backups stay on the same disk. Copy `.local/backups/` off the host if you need disaster recovery.
- Mise daemons are marked experimental upstream. Pin the `mise` version on the host and test upgrades before relying on them.
