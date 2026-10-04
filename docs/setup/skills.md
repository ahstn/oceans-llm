# Agent Skills

`See also`: [Configuration Reference](../configuration/configuration-reference.md), [Identity and Access](../access/identity-and-access.md), [Kubernetes and Helm](kubernetes-and-helm.md), [Self-hosting (mise daemons)](self-hosting-mise-daemons.md)

Oceans stores Agent Skills in an authenticated registry. Users can inspect instructions, browse bundled files, upload new versions, and install a selected version with the Oceans CLI. The gateway stores metadata in its database and ZIP archives in an S3 bucket. AWS S3 and RustFS use the same storage interface.

## Access and ownership

All authenticated users and API-key callers can read every skill and every version. Skills have no team grants, key grants, or private visibility setting.

An active user can claim one unique namespace. The namespace is chosen once and cannot be changed. A skill address combines that namespace with the skill name, such as `alice/code-review`. Another user can create `bob/code-review` without a conflict. Ownership follows the stable user ID, so a display-name change does not transfer a skill.

Namespaces and skill names use 1–64 lowercase ASCII letters, numbers, or hyphens. They cannot start or end with a hyphen or contain consecutive hyphens.

| Caller | Read all skills | Claim a namespace | Create skills | Add versions or change the default |
| --- | --- | --- | --- | --- |
| Active user session | Yes | Own namespace | Own namespace | Own skills only |
| User-owned API key | Yes | Owner's namespace | Owner's namespace | Owner's skills only |
| Service-account API key | Yes | No | No | No |

API keys remain subject to the gateway's existing authentication rules. Admin roles do not override skill ownership. An upload is available to all authenticated callers as soon as it succeeds; there is no approval or publish stage.

## Enable storage

Skills are disabled by default. Create a bucket before enabling the feature, then add this section to the gateway configuration:

```yaml
skills:
  enabled: true
  storage:
    bucket: oceans-skills
    region: eu-west-2
    prefix: skills/
  limits:
    max_archive_bytes: 10485760
    max_expanded_bytes: 26214400
    max_files: 1000
```

For AWS S3, omit `endpoint` and let the SDK select the regional endpoint. When explicit credentials are absent, the SDK uses its default credential providers. This supports a configured AWS profile or credentials supplied to the gateway workload. Custom endpoints must use `skills.storage.endpoint`; AWS endpoint environment variables and profile overrides do not replace that setting.

The gateway identity needs `s3:PutObject`, `s3:GetObject`, and `s3:DeleteObject` for the configured object prefix. It does not create buckets or require bucket-list permissions. Use a separate setup identity to create the bucket and provision access.

For RustFS or another compatible S3 endpoint, configure the endpoint and path-style addressing:

```yaml
skills:
  enabled: true
  storage:
    bucket: oceans-skills
    region: us-east-1
    endpoint: env.OCEANS_SKILLS_S3_ENDPOINT
    prefix: skills/
    force_path_style: true
    access_key_id: env.RUSTFS_ACCESS_KEY
    secret_access_key: env.RUSTFS_SECRET_KEY
```

Use HTTPS for remote storage. For a local or trusted internal HTTP endpoint, also set `skills.storage.allow_http: true`. The endpoint must contain only an origin, such as `http://127.0.0.1:9000`; paths, query strings, embedded credentials, and fragments are rejected.

Set `access_key_id` and `secret_access_key` together. An optional `session_token` also supports environment references, but requires both explicit credential fields. Supply secrets through the deployment environment; do not place them in checked-in YAML.

| Setting | Default | Requirement |
| --- | --- | --- |
| `enabled` | `false` | Enable after the bucket and credentials are ready. |
| `storage.bucket` | Unset | Required when skills are enabled. |
| `storage.region` | `us-east-1` | Match the bucket or compatible service. |
| `storage.endpoint` | Unset | Override for RustFS or another S3 endpoint. |
| `storage.prefix` | `skills/` | Relative object prefix without `.` or `..` segments. |
| `storage.force_path_style` | `false` | Use `true` for the RustFS examples. |
| `storage.allow_http` | `false` | Required for an HTTP storage endpoint. |
| `limits.max_archive_bytes` | `10485760` | Maximum ZIP size: 10 MiB. |
| `limits.max_expanded_bytes` | `26214400` | Maximum total extracted file size: 25 MiB. |
| `limits.max_files` | `1000` | Maximum number of files. |

All limits must be greater than zero. They are Oceans limits, separate from the Agent Skills specification's guidance on instruction length.

These limits apply to new uploads. Lowering a limit does not change stored versions: previews, downloads, and CLI installs use each version's recorded sizes and file count. Each download still verifies the archive size and SHA-256 digest.

### RustFS with mise daemons

The optional `rustfs` environment manages RustFS as a local runtime dependency. From the repository root, run:

```bash
mise -E rustfs run rustfs:setup
mise -E rustfs run rustfs:status
```

Setup creates local credentials, starts RustFS on a worktree-aware loopback port, and creates the `oceans-skills` bucket. The RustFS console is disabled. Keep the `rustfs` environment active when running the gateway so it receives the matching endpoint and credentials.

For a persistent self-hosted stack, use the `selfhost,rustfs,selfhost-rustfs` environments. The final environment gives RustFS a fixed port and persistent data path, and starts the bucket setup before the gateway. See [Self-hosting (mise daemons)](self-hosting-mise-daemons.md) for the complete environment, startup, and backup procedure.

### Curated skills in local testing

The repository keeps reviewed community copies under `bundled-skills/`, outside agent auto-load directories. The initial copy is Matt Pocock's self-contained `grill-me`, with its MIT license and a source link pinned to the reviewed commit. Local verification imports it into a temporary admin's `demo-admin` namespace. This ordinary user is separate from the bootstrap admin, which is excluded from API-key owner selection.

To import the copies into an existing local or self-hosted gateway, first enable Skills storage and create a user-owned API key in the UI. Supply that key as `OCEANS_API_KEY` through your private environment, then run:

```bash
export OCEANS_URL=http://127.0.0.1:8080
export OCEANS_SKILLS_NAMESPACE=my-skills
mise run skills:import-bundled
```

For the self-host environment, use `mise -E selfhost,rustfs,selfhost-rustfs run skills:import-bundled`. If the key's owner already has a namespace, set `OCEANS_SKILLS_NAMESPACE` to that exact value. Otherwise the task claims it once. Service-account keys cannot import skills.

This task runs explicitly; starting or upgrading the gateway does not import skills. It uses the normal authenticated upload path. An unchanged latest bundle is skipped; changed content appends a version and preserves the default. The duplicate check supports sequential local reruns, not concurrent production seed jobs. Production image packaging and managed library ownership are separate from this local task.

### RustFS with Helm

The Oceans chart accepts the official RustFS chart as an optional dependency. An admin supplies an existing CSI StorageClass and the required capacity through the RustFS values. Oceans does not install or manage a CSI driver.

Use the [Oceans Helm chart](../../deploy/helm/oceans-llm/README.md) for the dependency settings and deployment procedure. Complete values are available for [external S3](../../deploy/helm/oceans-llm/examples/skills-s3-values.yaml), [distributed RustFS](../../deploy/helm/oceans-llm/examples/skills-rustfs-values.yaml), and [standalone RustFS for development](../../deploy/helm/oceans-llm/examples/skills-rustfs-standalone-values.yaml).

Create the private bucket and a restricted application identity before enabling skill uploads. The Helm dependency does not provision either. External AWS S3 or an independently managed RustFS release can use the same gateway configuration without enabling the dependency.

## Prepare a skill

A skill follows the [Agent Skills specification](https://agentskills.io/specification). Its directory contains one root `SKILL.md` with YAML frontmatter:

```text
code-review/
├── SKILL.md
├── scripts/
├── references/
└── assets/
```

```markdown
---
name: code-review
description: Review code for correctness and maintainability. Use when reviewing a change before merge.
metadata:
  author: example-org
  version: "1.0"
  github: https://github.com/example-org/skills/tree/main/code-review
---

Read the change and its tests. Report defects with a clear reproduction path.
```

The required `name` matches the directory name. The required `description` explains the skill and when to use it. Optional frontmatter includes `license`, `compatibility`, `metadata`, and `allowed-tools`. Scripts, reference documents, and assets remain part of the same versioned bundle.

Optional `metadata.author`, `metadata.version`, and `metadata.github` provide attribution. The author is the original creator, which can differ from the user who owns the skill in Oceans. The upstream version is descriptive text, separate from Oceans' immutable integer versions; it does not need to follow semantic versioning. Do not invent an upstream version when the source does not provide one. Use a GitHub URL pinned to a commit when recording a curated copy's source.

The upload API accepts a ZIP with `SKILL.md` at its root or within one enclosing skill directory. Oceans validates the layout and stores a normalized ZIP with the enclosing directory. The recorded SHA-256 digest describes that stored archive, so it can differ from the digest of the uploaded ZIP.

Validation rejects unsafe paths, duplicate paths, links, invalid frontmatter, and bundles that exceed the configured limits. The gateway does not execute uploaded scripts. Review a skill's instructions and scripts before running them in an agent.

## Use the Skills UI

Open **Skills** at `/admin/skills` in the signed-in UI to browse the shared catalog. Claim your namespace before the first upload. Upload a ZIP to create a skill, then open the skill to inspect its instructions, file list, and versions. The detail view renders `SKILL.md` without raw HTML or remote images and shows other text files as plain text.

The selected version shows its author and upstream version when those values are present and nonempty. A valid HTTPS GitHub repository URL appears as a source link. Missing attribution and invalid links do not prevent viewing a skill. Attribution comes from the uploaded manifest; it is not proof that the named creator published or endorsed that copy.

Only the owner can add a version or select a different default. The detail view shows the namespace and version so skills with the same name remain distinct. A successful upload is immediately readable by other authenticated users.

## Use the Oceans CLI

Install the client from the repository root:

```bash
mise exec -- cargo install --locked --path crates/oceans-cli
```

Cargo installs the `oceans` executable into its binary directory. Set `OCEANS_URL` to the gateway URL and supply an existing user-owned key through `OCEANS_API_KEY`. The examples use `oceans` from your `PATH`. `--url` overrides the configured URL, and `--json` prints structured output. The CLI has no separate login flow.

```bash
export OCEANS_URL=https://oceans.example.com
oceans skills namespace alice
oceans skills upload ./code-review
oceans skills list
oceans skills show alice/code-review
oceans skills versions alice/code-review
```

`namespace` without a handle displays your current namespace. `upload` accepts either a directory or a ZIP. If your namespace already contains that skill name, the command adds an immutable version. To ingest a private Git source, clone it with your existing Git credentials and upload its skill directory.

Use `oceans skills upload --skip-unchanged ./code-review` to skip an upload when its normalized archive matches your skill's latest version. This does not select a new default or change ordinary upload behavior. Concurrent uploads can still append identical versions.

Version 1 becomes the default when a skill is created. Later uploads increase `latest_version` but leave `default_version` unchanged. All versions remain readable. Select a new default with:

```bash
oceans skills set-default alice/code-review 2
```

Download or install a specific version:

```bash
oceans skills download alice/code-review --version 2 --output code-review-v2.zip
oceans skills install alice/code-review --version 2
```

Without `--version`, these commands use the registry's current default. Downloads verify the archive digest and refuse to overwrite an existing output file. Installation also validates the bundle before writing files.

The default installation directory is `.agents/skills`, with the skill in `.agents/skills/code-review`. Set `--directory` for another agent's skill directory. The CLI records the namespace, skill ID, version, and digest in `.oceans-skill-lock.json` inside the installed directory. It does not store the API key there.

Installation refuses an existing destination by default. `--replace` replaces that skill directory, including local edits, and leaves sibling directories unchanged. Two owners' skills with the same name need separate installation directories because the Agent Skills directory name must match the manifest name.

## HTTP API

The UI and CLI use `/api/v1/skills`. Clients authenticate with `Authorization: Bearer <key>` or `x-oceans-api-key`. Browser sessions use the existing session cookie; write requests must come from the same origin. Requests with conflicting key headers are rejected.

| Method and path | Purpose |
| --- | --- |
| `GET /api/v1/skills` | List skills; accepts `namespace`, `limit` from 1 to 100, and `offset`. |
| `GET /api/v1/skills/limits` | Read the configured archive, expanded-size, and file-count limits. |
| `POST /api/v1/skills` | Create a skill in the caller's namespace from a ZIP body. |
| `GET /api/v1/skills/namespace` | Read the caller's namespace, or `null` when none exists. |
| `POST /api/v1/skills/namespace` | Claim a namespace with `{"handle":"alice"}`. |
| `GET /api/v1/skills/by-name/{namespace}/{name}` | Resolve a qualified name to skill metadata and versions. |
| `GET /api/v1/skills/{id}` | Read skill metadata and versions by UUID. |
| `GET /api/v1/skills/{id}/versions` | List immutable versions. |
| `POST /api/v1/skills/{id}/versions` | Add a ZIP version to an owned skill. |
| `GET /api/v1/skills/{id}/versions/{version}` | Read the manifest, file list, and `SKILL.md` content. |
| `GET /api/v1/skills/{id}/versions/{version}/archive` | Download the normalized ZIP. |
| `GET /api/v1/skills/{id}/versions/{version}/files?path=references/guide.md` | Read one UTF-8 text file. |
| `PUT /api/v1/skills/{id}/default-version` | Select an owned skill's default with `{"version":2}`. |

Uploads use `Content-Type: application/zip` with the archive as the request body, not multipart form data. Successful upload responses include `uploaded_version`, which identifies the version allocated to that request even if another upload completes at the same time. JSON requests use `Content-Type: application/json`. Archive downloads include `x-skill-sha256` and disable shared caching. Binary files are available in the archive but cannot be previewed through the text-file endpoint.

## Operate and recover

Back up both the gateway database and the configured bucket. The database contains ownership, namespaces, version metadata, and object references. The bucket contains archive bytes. Restoring only one side can leave versions without their archives.

An upload writes the object before recording the version. A known metadata conflict triggers cleanup of the unused object. If the database write has an uncertain outcome, Oceans keeps the object and logs its key for reconciliation. This prevents a failed response from deleting an archive whose metadata may have committed. There is no automatic orphan cleanup job.

The gateway checks the stored digest before returning an archive. If that check fails, restore the matching object from backup and investigate the storage change. Do not replace immutable version bytes with different content.

Changing the bucket or prefix does not move existing archives. Copy the referenced objects and retain a matching database backup before changing either setting. Protect the bucket from public reads; callers download through the authenticated gateway.

## Compatibility boundaries

Oceans distributes skill bundles. It does not automatically register them with a model provider or attach them to an inference request. Installation and execution remain the responsibility of the selected agent.

The current API does not provide server-side Git imports, public discovery indexes, or a Claude Code marketplace. Use the Oceans CLI for authenticated downloads. The shared bundle format remains compatible with the Agent Skills directory specification.
