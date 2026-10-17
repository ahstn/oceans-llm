# ADR: MCP Tag Grants, Annotation Overrides, and Tool Versioning

- Date: 2026-10-17
- Status: Accepted
- Issues: [#443](https://github.com/ahstn/oceans-llm/issues/443) and [#444](https://github.com/ahstn/oceans-llm/issues/444), part of [#458](https://github.com/ahstn/oceans-llm/issues/458)

## Current state

- [MCP Tool Access: Tag rules](../mcp/mcp-tool-access.md#tag-rules)
- [MCP Tool Access: Tool versions and approval](../mcp/mcp-tool-access.md#tool-versions-and-approval)

## Context

Grants name tools one at a time, directly or through a toolset. Giving a team "every read-only tool on this server" means picking tools by hand and checking again after every discovery. There is also no single rule that says "never allow destructive tools for service accounts".

Discovery stores each tool's `title`, `annotations` and `outputSchema` and hashes them with the input schema ([Store MCP Tool Metadata in the Schema Hash](2026-10-13-mcp-tool-metadata-and-schema-hash.md)). The annotations are claims made by the upstream server, so they cannot be trusted as policy on their own.

A schema change replaces the stored definition, and the new definition is served at once. An upstream server can change a tool description to inject instructions, or widen what a tool accepts, after an admin has reviewed and granted it. Nothing records what a tool looked like when an earlier call was made.

#443 and #444 both change the tool data model and the effective tool set, so they are designed together. Three questions decide how they fit:

1. Does an admin annotation override create a tool version? **No.**
2. Do tag rules match tools that are waiting for approval? **No.**
3. Which annotation values feed the schema hash? **The discovered values.**

## Decision

### 1. Effective annotations and tags (#443)

`external_mcp_tools` gains two admin-owned columns that discovery never writes:

- `annotation_overrides_json`: an object that may set any of `readOnlyHint`, `destructiveHint`, `idempotentHint` and `openWorldHint` to a boolean. Other members are rejected.
- `admin_tags_json`: a sorted array of admin tags.

The discovered `annotations_json` is unchanged, so the admin UI can show the upstream value next to the override.

A tool's **effective hints** are the override where one is set, otherwise the discovered value, otherwise the protocol default. Its **derived tags** come from the effective hints:

| Tag | Present when |
|---|---|
| `read_only` | `readOnlyHint` is `true` (default `false`). |
| `destructive` | the tool is not read-only and `destructiveHint` is not `false` (default `true`). |
| `idempotent` | `idempotentHint` is `true` (default `false`). |
| `open_world` | `openWorldHint` is not `false` (default `true`). |

The defaults follow the protocol, and they fail safe. An include rule matches only a positive claim, so `read_only` needs an explicit `readOnlyHint: true`. An exclude rule catches a tool that declares nothing, so a tool without hints counts as `destructive` and `open_world`.

Admin tags are 1 to 64 characters of lowercase letters, digits, `_`, `-`, `.` and `:`, with at most 32 per tool. The four derived names are reserved, so an admin tag cannot claim `read_only` for a tool whose hints say otherwise.

The evaluation is one pure function in `gateway_core::mcp_tool_policy`. The store, the service and the admin preview all call it, so they cannot disagree.

### 2. Tag rules (#443)

A new table `mcp_tool_tag_rules` holds rules for the same subjects as grants (API key, user, team and service account):

- `effect` is `include` or `exclude`.
- `server_scope` is a server ID, or `*` for every server. Only exclude rules may use `*`. An include rule across all servers would grant every tool on every future server, which is too broad for one click.
- `tag` is a derived tag or an admin tag.
- Rules are revoked rather than deleted, like grants. One active rule may exist per subject, server scope, tag and effect.

Resolution for a caller, in `resolve_mcp_access_for_subjects` and `resolve_mcp_catalog_access_for_subjects` on both backends:

1. Collect the explicit tool and toolset grants of every subject, as before.
2. Add every served tool on an include rule's server whose effective tags contain the rule's tag. Tools discovered later match without a new grant.
3. Remove every tool that matches an exclude rule of **any** of the caller's subjects. Exclude wins over explicit grants, toolset grants and include rules, whichever subject they came from. "Never allow destructive tools for service accounts" is then one exclude rule on the service account.

Because all three steps run inside the two resolution methods, every surface that already used them follows the rules: direct `tools/list` and `tools/call`, aggregate `search_tools`, `describe_tool` and `call_tool`, and the admin effective-access preview.

`has_mcp_server_grant`, which gates resources, prompts, completions and the event stream on the direct proxy ([Govern Every MCP Method](2026-10-13-govern-every-mcp-method.md)), keeps its single `EXISTS` query when the caller's subjects have no active tag rules. With rules, it runs the full resolution for that server and checks that at least one tool is left. An exclude rule can therefore remove a caller's last tool on a server and with it their access to that server's resources and prompts. An include rule can give that access.

The access projection now also loads `annotations_json` and the two admin columns, because tag evaluation needs them. It still leaves out the title and output schema.

Whether the caller's subjects have any rule is a column on queries the resolution already runs (the tool count, and the server grant check), so callers without rules make no extra round trip. Rules are loaded only when that column is true. An include rule scans its servers' tools with only the columns tag evaluation reads (no schemas or text), then loads full rows for the matching tools alone. Effective tags depend on overrides and protocol defaults, so they are evaluated in Rust rather than indexed; the scan stays linear in a server's tool count but carries no schemas.

The rule preview reads candidates a page at a time and stops after 200 matches or after checking 5,120 tools. It reports `truncated` when it stopped early, so a preview for a rare tag on a large registry is bounded and says it may be incomplete.

### 3. Overrides are served to clients (#443)

Admin-set annotations take precedence over the upstream's. `describe_tool` and `search_tools` return the effective annotations. Direct `tools/list` merges the override hints into each granted tool's `annotations` object and keeps every other member as the upstream sent it. Clients that ask for confirmation before destructive tools then see the admin's answer.

### 4. Overrides do not create versions, and the hash uses discovered values

The schema hash and versions describe what the upstream declared. They are how the gateway notices that the upstream changed. An override is a local policy decision, so it:

- does not change `schema_hash` or `schema_version`,
- does not create a tool version and never needs approval, and
- is recorded in the admin audit log instead (`mcp_tool.annotations_overridden`).

`schema_hash` is the hash of the discovered definition. A `call_tool` caller that passes `schema_hash` is checked against that hash, as before.

### 5. Tool versions (#444)

A new table `mcp_tool_versions` keeps every definition the upstream served for a tool, keyed by `(mcp_tool_id, schema_version)`: the hash, description, input schema, title, annotations, output schema, discovery time, `status`, and who reviewed it and when. Discovery writes a version row whenever a tool's definition differs from every version that tool already has. Two definitions are the same when their schema hash, description and title all match. The schema hash leaves the description out, and a changed description is exactly the injection this protects against, so the description is compared on its own. The hash covers the title today, but the title is compared directly as well so that versioning never depends on what the hash happens to cover. Servers on the older encoding that put the title in `annotations.title` change their annotations with every title change, so the legacy `title` member is ignored when deciding whether a change kept the call contract. The migration backfills one `approved` version for each existing tool.

Discovery plans each tool's writes from a snapshot of its versions, so it is serialized with reviews and with other discoveries of the same server. On libSQL, discovery runs in an immediate transaction that holds the write lock. On Postgres, it locks the server row and then the server's tool rows before taking the snapshot, and a review locks the tool row it changes.

Statuses are `approved`, `pending`, `rejected` and `superseded` (a pending version replaced by a newer discovery, or by the upstream going back to the approved definition).

### 6. Approval of changed tools (#444)

`external_mcp_servers.approve_tool_changes` is off by default. When it is off, discovery approves each new version automatically (`reviewed_by_user_id` is empty), so behaviour is as before.

When it is on:

- The tool row keeps serving the last approved definition. A new definition is stored only as a `pending` version, and the tool's `approval_status` records what is waiting.
- A **newly discovered tool** is `pending_review`: not listed, not searchable, not callable.
- A **changed tool whose description or title changed and nothing else** is `pending_change`. The previous approved version keeps being served and callable, because the call contract is unchanged. A changed description is the injection vector, and the client never sees it until it is approved.
- A **changed tool whose input schema, output schema or annotations changed** is `pending_review` and is withheld. The old definition no longer describes what the upstream accepts or claims, so serving it is not safe.
- **Approve** copies the version into the tool row, makes it the served definition, and marks older pending versions `superseded`.
- **Reject** marks the version `rejected`. A text-only change goes back to `approved` and keeps serving the previous version. A new tool, or a rejected contract change, becomes `rejected` and stays withheld until a later discovery brings a definition an admin approves. Discovery does not reopen a rejected definition. If the upstream serves a rejected definition again, any newer pending version is superseded, because the upstream no longer offers it, and the tool settles to `approved` when the rejected change kept the contract of the served version, or `rejected` otherwise. A tool that was never approved holds its newest definition in the row, so settling it also puts the rejected definition and its version number back in the row. Otherwise a superseded definition would pass for the current one and could never be offered for review again.
- If the upstream goes back to the approved definition, the pending version is `superseded` and the tool is `approved` again.
- If the upstream goes back to an older definition, discovery reuses that version instead of creating a duplicate. A version an admin approved before is served again without review. One that was superseded before review waits again under its old number. Discovery looks this up only when it would otherwise create a version, so it does not read the whole history on every refresh. The admin API reads history the same way: the versions endpoint returns either the versions named in `schema_versions` (at most 10, which the review diff uses for the approved and pending versions) or one page of at most 100, newest first, with `next_before` for the next page.

Turning the setting off does not approve anything on its own. The next discovery approves the current definitions. Turning it on leaves existing tools approved.

**Served** means `approval_status` is `approved` or `pending_change`. Every resolution query filters on it next to `is_active`, so pending and rejected tools are excluded from explicit grants, toolset grants and tag-rule matches alike (question 2). They cannot be matched by an include rule before review.

For servers with approval on, direct `tools/list` replaces each granted tool's object with the stored approved definition (with effective annotations). Discovery runs only when an admin refreshes, so the live upstream list could otherwise show a changed description before anyone reviewed it. Servers without approval keep the upstream objects, as before, except for a tool whose change still waits or was withheld: after approval is turned off, that tool keeps its stored definition until the next discovery approves what the upstream serves.

### 7. Invocation log (#444)

`mcp_tool_invocations` gains `tool_schema_version`, the served version at the time of the call. It is set for `tools/call` whenever the client saw the stored definition: always on the aggregate endpoint, whose `describe_tool` serves it, and on the direct endpoint when the stored definition replaced the upstream object. A direct call on a server without approval passes the live upstream definition through, which may have changed since the last discovery, so it records no version. Other methods record none. The admin invocation detail shows it.

### 8. Audit and permissions

Every change goes through `AdminAudit` with new constants:

- target `mcp_tool`: `mcp_tool.tags_updated`, `mcp_tool.annotations_overridden`, `mcp_tool.version_approved`, `mcp_tool.version_rejected`
- target `mcp_tag_rule`: `mcp_tag_rule.set`, `mcp_tag_rule.revoked`
- the server setting is part of `mcp_server.updated`

The new admin handlers check permission through one function, `require_mcp_policy_admin`, which takes the server ID the change applies to. Today it requires a platform admin, like grants. Delegated server ownership (#445) can widen it in one place.

## Trade-offs

- Tag rules are evaluated in Rust after loading the candidate tools of each include rule's server, not in SQL. One query per rule server is cheap at registry sizes, and both backends share the evaluation. A server with thousands of tools would load them all for each resolution.
- An exclude rule on a team also removes tools that were granted to a member's own key. That is the intent of a deny rule, but it can surprise someone who reads only the key's grants. The effective-access preview shows the result.
- `pending_change` keeps a tool callable while a text change waits. The upstream's behaviour may already have changed with its description. Admins who want every change held can reject text-only changes, which keeps the old text, or disable the server.
- With approval on, direct `tools/list` drops upstream members the gateway does not store, such as `_meta` and `icons`, for every tool.
- Discovery only runs on refresh, so a change made upstream between refreshes is not seen until the next one.

## Follow-up

- Run discovery on a schedule, so changes are noticed without an admin refresh.
- Let delegated server owners (#445) manage tags, overrides, rules and approvals for their servers through `require_mcp_policy_admin`.
- Consider versioning access policies, including tag rules, with history and rollback (#447).
