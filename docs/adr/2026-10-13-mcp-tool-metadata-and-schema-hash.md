# ADR: Store MCP Tool Metadata in the Schema Hash

- Date: 2026-10-13
- Status: Accepted

## Current state

- [MCP Servers: Discovery](../configuration/mcp-servers.md#discovery)
- [MCP Tool Access: Runtime Behavior](../mcp/mcp-tool-access.md#runtime-behavior)

## Context

Discovery kept only each tool's `name`, `description`, and `inputSchema`. Upstream servers can also declare:

- a display `title`
- an `outputSchema` for structured results
- `annotations`, such as `readOnlyHint`, `destructiveHint`, `idempotentHint`, and `openWorldHint`

Aggregate `/mcp` clients read tools through `describe_tool`, so they never saw these members. Admins could not tell which tools claim to be read-only or destructive when they built toolsets. A change to these members did not count as drift either, even though it can change how a client or reviewer treats the tool.

## Decision

### 1. Store the three members as discovered

Migration `V58__mcp_tool_metadata` (libSQL and Postgres) adds `title`, `annotations_json`, and `output_schema_json` to `external_mcp_tools`. Each discovery overwrites them.

Annotations are stored as the upstream JSON object. Members that Oceans does not interpret are kept as well. The admin API exposes the four protocol hints as typed booleans. `describe_tool` and `search_tools` return the stored object.

Normalization trims the title and drops it when it is empty. It drops `outputSchema` and `annotations` when they are not JSON objects. A malformed optional member does not fail discovery for the rest of the server.

### 2. Include them in the schema hash

`tool_schema_hash` hashes a canonical envelope of `inputSchema`, `title`, `annotations`, and `outputSchema`. Members that are absent are left out of the envelope. The envelope hash is prefixed with a fixed domain tag, so an input schema that happens to look like an envelope cannot share a hash with a tool that declares metadata. When a tool declares none of the three, the hash is the input-schema hash used before, so those tools keep their hash.

The existing drift rules apply. A new hash bumps `schema_version`, and `call_tool` rejects a stale caller-supplied `schema_hash` with `tool_schema_changed`.

### 3. Do not count the first capture as drift

Rows that existed before V58 get `metadata_captured = 0`. On the first discovery after the upgrade, the upsert updates the hash. It does not bump `schema_version` when the stored input schema is unchanged. Every upsert sets `metadata_captured = 1`, so later changes count as drift.

Without this rule, every annotated tool would report a new schema version after the upgrade, though nothing upstream changed.

### 4. Gateway tools declare their own hints

The aggregate `search_tools` and `describe_tool` definitions declare `readOnlyHint`, `idempotentHint`, and `openWorldHint: false`. `call_tool` declares no hints, because its effect depends on the tool it calls.

### 5. The direct proxy keeps passing tools through

The direct `tools/list` filter already removed tools by name and left the remaining tool objects unchanged. A test now proves that granted tools keep `title`, `annotations`, `outputSchema`, and other members.

## Trade-offs

- Callers that cached a schema hash for an annotated tool get one `tool_schema_changed` after the upgrade. They need to call `describe_tool` again. The version number does not change, but the hash must, or later metadata changes could not be detected.
- Hints are claims from the upstream server. Oceans displays and returns them but does not enforce them.
- An annotation-only change, such as a new `x-` member, is reported as drift even if it does not affect behavior.

## Follow-up

- Admin overrides for annotations and grant rules based on hints, such as denying destructive tools for a subject, are tracked in #443.
