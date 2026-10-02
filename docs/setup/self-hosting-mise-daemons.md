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

### 5. First deploy

```sh
mise run deploy
```

This builds the UI and the release gateway, starts Postgres, and starts the gateway and UI. Migrations, config seeding, and bootstrap admin run on gateway start. The first Rust build is slow.

### 6. Expose on the tailnet

```sh
sudo tailscale set --operator="$USER"
tailscale serve --bg --https=443 http://127.0.0.1:8080
tailscale serve status
```

Serve config persists across reboots. Do not run `tailscale funnel`. Funnel makes the service public. Limit who can reach `hogwarts:443` with Tailscale ACLs.

Sign in at `https://hogwarts.<tailnet>.ts.net/admin` as `admin@local` with `GATEWAY_BOOTSTRAP_ADMIN_PASSWORD`. The gateway forces a password change.

### 7. Start at boot

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
5. Dumps the database to `.local/backups/` (mode 600, kept 30 days). It skips this when Postgres is not running.
6. Runs `gateway config validate`.
7. Re-registers daemons, makes sure `db` runs, and restarts `gateway` and `admin-ui`.
8. Checks `/readyz` and `/admin/login` on the loopback listener.

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
