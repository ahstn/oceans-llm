# ADR: Authenticated Agent Skills with S3 Storage

- Date: 2026-10-03
- Status: Accepted

## Current state

- [Agent Skills](../setup/skills.md)
- [Self-hosting with mise daemons](../setup/self-hosting-mise-daemons.md)
- [Oceans Helm chart](../../deploy/helm/oceans-llm/README.md)

## Context

Oceans needs a shared skill catalog, a UI for inspection and upload, and a client that can upload and install skills through authenticated HTTP requests. The gateway already has identity, SQL persistence, and service boundaries. It has no general object store or remote administration CLI.

The [Agent Skills specification](https://agentskills.io/specification) defines a directory with `SKILL.md` and optional resources. Its instruction-length guidance does not limit the total archive size. Skills can contain executable scripts and binary assets, so the storage and validation rules must cover the complete bundle.

The initial research considered skill-specific grants and a publish stage. The accepted first release instead lets every authenticated caller read every skill. Active users can create and change their own skills. Service accounts have read access only. Two users must be able to use the same skill name.

Oceans user records have stable IDs and unique email addresses, but display names are not unique. A display name cannot serve as an ownership key. The public address also needs to survive a display-name or email change.

## Decision

### Identity and versions

Each active user can claim one unique, immutable skill namespace. The public address is `namespace/skill-name`; database ownership uses the stable user UUID. A uniqueness constraint applies to the owner and skill name. The namespace is an address, not proof of ownership.

Authenticated sessions and API keys can read the full catalog, previews, and archives. Only a user session or user-owned API key can create a skill or modify its own skill. Service-account keys cannot write. Platform and team admin roles do not bypass ownership. There are no subject grants, approval states, or publish states in this release.

Skill versions are immutable positive integers. The first upload sets both the latest and default version to 1. Later uploads only advance the latest version. The owner can move the default pointer to an existing version. Every version is readable, so the default pointer controls selection rather than access.

### Bundle format and validation

Accept a local directory through the CLI or a raw ZIP through the API and UI. A ZIP can contain a root `SKILL.md` or one enclosing skill directory. Normalize the result to one enclosing directory named after the manifest. Store a SHA-256 digest of the normalized archive.

The shared validator checks frontmatter, layout, paths, file count, and expanded size. It rejects links, traversal, duplicate paths, and other invalid bundle entries. The initial defaults are 10 MiB compressed, 25 MiB expanded, and 1,000 files. These are configurable Oceans limits, not claims about the format specification. The gateway never executes bundle content.

Server-side Git import is deferred. Users can clone a private source with their existing Git credentials and upload the selected directory. This avoids a new credential-retention and remote-fetch service in the first release.

### Storage and failure handling

Keep namespaces, ownership, version metadata, file manifests, and object references in the existing SQL store. Supply migrations and repository implementations for both libsql and PostgreSQL. Store archive bytes in S3 from the start.

Each libSQL skill operation uses its own connection so it cannot join another request's transaction. All connections enforce foreign keys and wait up to five seconds for a busy database. Skill transactions contain only database work; archive validation and S3 requests take place outside them.

Use one S3 adapter for AWS S3 and RustFS. Configuration selects the bucket, region, object prefix, endpoint, and path-style addressing. The adapter uses the AWS Rust SDK for request signing and credential providers. The SDK explicitly supports [custom endpoints and path-style addressing](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/endpoints.html); Oceans does not implement S3 signing itself.

Each upload gets a new object key. Write the archive first, then commit the skill metadata. On a definite logical rejection, remove the unused object. On an ambiguous database failure, retain the object and report its key for manual reconciliation. A database error can occur after a commit, so unconditional deletion could remove a referenced archive. SQL and object storage do not share a transaction.

Download through the authenticated gateway and verify the stored size and digest before returning bytes. The first release does not issue presigned download URLs. Database and object-store backups form one recovery set.

### Crate boundaries

Add two workspace packages and keep the other responsibilities in their existing layers:

| Package | Responsibility |
| --- | --- |
| `gateway-skills` | Shared bundle parsing, validation, normalization, and API data types. No database, HTTP server, identity service, or S3 client. |
| `oceans-cli` | Remote client commands, API-key transport, local packaging, downloads, and installation. |
| `gateway-core` | Skill repository and object-store contracts plus persistence records. |
| `gateway-store` | SQL repositories, migrations, and the S3 object adapter. |
| `gateway-service` | Ownership checks, version workflows, and coordination between SQL and object storage. |
| `gateway` | HTTP authentication and routes, request limits, response mapping, configuration, and runtime composition. |

The HTTP handlers remain in `gateway::http::skills`. The CLI must not depend on the gateway runtime or database layers. The shared bundle library prevents the client and server from maintaining separate validation rules.

This is a dependency boundary, not a file-size split. Both callers need the bundle model, while only the server needs persistence and authorization. Cargo workspaces support [explicit dependencies between related packages](https://doc.rust-lang.org/stable/book/ch14-03-cargo-workspaces.html) with a shared lockfile. Cargo also permits a binary in an existing package; a separate client package is selected here to keep the CLI's dependency set and release entry point distinct.

Use ordinary Rust modules, typed errors, explicit ownership, and narrow public interfaces. Keep SDK and database errors inside their adapters. Perform bounded archive work outside asynchronous request execution. Add traits at the storage boundary where tests and adapters need them; avoid a generic plugin framework for one bundle format.

### Local and Kubernetes runtime dependencies

Manage optional local RustFS with `mise daemons`, alongside the existing PostgreSQL dependency. Use a separate mise environment so normal development and external-S3 deployments do not start RustFS. The self-hosted overlay gives RustFS persistent storage and starts bucket setup before the gateway.

For Kubernetes, depend on the official [RustFS Helm chart](https://charts.rustfs.com/). Supply Oceans example values for gateway configuration, endpoint wiring, and credentials. Admins create the bucket and restricted application identity before enabling uploads. End users supply the CSI StorageClass and capacity. Oceans does not create a CSI implementation or duplicate RustFS workload templates.

### Client and discovery compatibility

Use the Oceans CLI for authenticated HTTP access. As reviewed on 2026-10-03, the Vercel `skills` CLI can use private Git credentials, but its HTTP source path does not meet Oceans' token-authentication requirement. The [upstream request for private HTTP source authentication](https://github.com/vercel-labs/skills/issues/1031) remains open. Do not add an unauthenticated endpoint to accommodate that client.

The proposed [well-known discovery specification](https://github.com/agentskills/agentskills/pull/254) is still an open pull request. Public indexes, marketplace manifests, MCP exposure, and provider-hosted execution are deferred compatibility adapters. They are not part of the storage or ownership model.

## Consequences

The design gives all authenticated users a shared catalog while keeping ownership enforcement simple. Namespaces avoid conflicts without using email addresses in public skill paths. Immutable versions and digests support reproducible installs, and the client and server share bundle rules.

S3 becomes a required dependency when skills are enabled. The feature remains disabled by default. RustFS provides a local S3 path, but admins remain responsible for persistent volumes, credentials, backups, and restore tests.

An owner can upload content that every authenticated user can read immediately. There is no approval queue or private draft state. Validation protects the archive boundary; it does not establish that a skill's instructions or scripts are safe to execute.

Keeping a default pointer separate from the latest upload gives owners control over routine installs. It does not hide other versions. Deletion, ownership transfer, namespace changes, and automated orphan cleanup are outside this release.

## Follow-up work

- Add a reconciliation workflow for unreferenced objects after ambiguous failures, with protection for in-progress uploads.
- Decide whether deletion and retention need tombstones before exposing a delete API.
- Add server-side source imports only with bounded fetching, source pinning, and an explicit credential policy.
- Recheck authenticated client compatibility before adding discovery or marketplace exports.
- Add provider adapters separately if hosted skill execution becomes a requirement.

## Attribution

This ADR records the design agreed during collaborative human and AI implementation work.
