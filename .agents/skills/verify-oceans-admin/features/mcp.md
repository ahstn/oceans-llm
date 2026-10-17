# MCP

MCP verification follows the admin browser path from server registration to tool access. It checks the Tool Sets workbench, generated client configuration, direct and aggregate tool calls, upstream authentication failure, and invocation records against the real local gateway.

## Sub-features

- `mcp-registry` creates servers, refreshes discovery, and compares tool rows and registry counts with the production API. The `Auth mode` select labels `gateway_static_header` as `Gateway static credential`; its auth config takes `header_name` with an optional `value_template`, or `query_param`.
- `mcp-oauth-discovery` is the `Discover OAuth` button under `Server URL` in `Add server` and `Edit MCP server`. It calls `POST /api/v1/admin/mcp/oauth-discovery` and renders the result in `mcp-oauth-discovery`. The result shows a registration badge (`Dynamic client registration`, `Client metadata document`, or `Admin-supplied client ID`), the endpoints and default scopes, and a `Check before saving` alert for warnings. `Use discovered settings` sets `OAuth on-behalf-of` and fills `Auth config JSON`. A failure shows `OAuth discovery failed`.
- `mcp-toolsets` saves membership, checks it after reload, keeps drafts independent between two sets, confirms an empty replacement, and preserves membership during a metadata edit.
- `mcp-client-config` compares each Connection Info client block with the backend response and checks the workbench at a 390-pixel viewport.
- `mcp-grants` proves denial before an explicit tool-set grant and confirms the exact effective tool list after the grant.
- `mcp-tool-reviews` is manual. `Edit MCP server` and `Configuration` have a `Require approval for tool changes` switch (`mcp-approve-tool-changes`). On such a server, tool rows show `Pending review`, `Change pending` or `Rejected` instead of `Active`. The server dialog's `Reviews` section (`mcp-tool-reviews`) lists each waiting tool (`mcp-tool-review`) with `Version <n> → <m>` or `New tool, version <n>`, a `Served as approved version` or `Withheld` badge, one diff block per changed section (`mcp-tool-diff`, lines marked `data-diff="added"` or `"removed"`), and `Approve` and `Reject`. The MCP invocation detail has a `Tool version` row. Check `/api/v1/admin/mcp/servers/{id}/tools/{tool_id}/versions` and the audit entries `mcp_tool.version_approved` and `mcp_tool.version_rejected`.
- `mcp-tag-rules` is manual. Expanded tool rows in `mcp-server-tools` hold `mcp-tool-policy`: `Access tags` (`mcp-tool-effective-tags`), `Annotation overrides` (`mcp-tool-annotation-overrides`, one `<hint> override` select per hint with `Use upstream`, `Yes`, `No`, next to `Upstream: Yes`, `No` or `Not declared`) with `Save overrides`, and `Admin tags` with `Save tags`. The Access tab has a `Tag rules` section (`mcp-tag-rules`) with `Tag rule subject type`, `Tag rule subject`, `Tag rule effect`, `Tag rule server`, `Tag rule tag`, `Preview matches` (`mcp-tag-rule-preview`), `Save rule`, and the active list `mcp-tag-rule-list` with `Revoke`. Check the API through `/api/v1/admin/mcp/tag-rules` and `/api/v1/admin/mcp/effective-access`, and the audit entries `mcp_tool.tags_updated`, `mcp_tool.annotations_overridden`, `mcp_tag_rule.set` and `mcp_tag_rule.revoked`.
- `mcp-routing` calls one reviewed tool through each direct route and through aggregate `search_tools`, `describe_tool`, and `call_tool`.
- `mcp-upstream-auth` compares a valid Exa key with an invalid-key control and requires a recorded authentication error from the control.
- `mcp-invocations` matches calls to request-linked invocation records, then checks the filtered list and detail in the UI. Each row has a `Method` column, filter (`mcp-filter-method`), and detail row holding the JSON-RPC method, such as `tools/call` or `resources/read`. The `Target` column, filter, and detail row hold the tool name, resource URI, or prompt name.
- `mcp-cleanup` revokes the grant and API key, checks HTTP 401, and disables the servers and tool sets created by the run.

## How to get to it (user POV)

- Open `/admin/mcp` and sign in as the seeded platform administrator. The registry is also available at `/admin/mcp?tab=servers`.
- The `MCP sections` nav switches between `Servers`, `Tool Sets` (`/admin/mcp/toolsets`), and `Access` (`/admin/mcp?tab=access`). `/admin/mcp/servers`, `/admin/mcp/access`, and `?tab=toolsets` all redirect.
- Open `/admin/mcp/toolsets` for the Tool Sets workbench and its `Connection Info` dialog.
- Open `API Keys` at `/admin/api-keys` to create the temporary caller key, then `/admin/mcp?tab=access` to grant the saved tool set.
- Connect an MCP client to `/mcp/{server_key}` for one server or `/mcp` for the aggregate catalog.
- Open `MCP Invocations` in the `Observability` sidebar group at `/admin/observability/mcp-invocations` (page heading `MCP invocations`) to inspect a call.

## Driving it with control-oceans-admin

Preconditions:

- Use the owned local stack and require `control-oceans-admin doctor` to pass. The platform administrator needs the `mcp`, `api_keys`, and `mcp_invocations` pages plus `create_api_key` and `revoke_api_key` actions. Root `gateway.yaml` grants these through the configured permission groups.
- `launch` sets `GATEWAY_CLIENT_CONFIG_BASE_URL` to the run's gateway URL unless you export another origin first. This value controls generated client configuration. The driver expects the endpoint the gateway generates from it (trailing `/` and a final `/v1` removed, then `/mcp`), so a candidate file needs `expected_gateway_url` only to assert a different value.
- Load the required credential aliases into the gateway before launch, either exported in the launching shell or set in a gitignored `mise.local.toml`. The gateway reads them only at startup. `launch` records which `OCEANS_MCP_DISCOVERY_*` names its stack environment resolved (names only, never values) in `stack-env.json`, and the driver marks any candidate whose alias is missing from that record `blocked`. Exporting an alias after launch, or only in the shell that runs `drive`, has no effect; relaunch instead. The example uses the GitHub token from `gh auth token --hostname github.com` and Exa's `EXA_API_KEY` from Mise. Keep their values in process memory and out of the candidate file and evidence. Without GitHub or Exa keys, copy the example, set `required: false` on `github` and `exa` (or remove them), and report them as gaps.
- Rerunning `drive mcp` in the same run is safe. Each attempt adds its own scope to every server key, tool set key, and API key name (`verify-<run tail>-<attempt>-…`), so it never collides with records an earlier attempt disabled and retained.
- Use only reviewed read-only tools with public or synthetic arguments. The example sends two successful upstream tool calls per positive candidate, plus one failing aggregate call for the invalid-key control. Discovery and protocol setup add requests. MCP calls can consume service quota; no LLM provider request is sent.

The [candidate example](../examples/mcp-candidates.json) uses these endpoint and authentication settings:

| Candidate | HTTPS endpoint | Auth mode | Auth config |
| --- | --- | --- | --- |
| Context7 | `https://mcp.context7.com/mcp` | `none` | `{}` |
| GitHub | `https://api.githubcopilot.com/mcp/x/repos/readonly` | `gateway_bearer_token` | `secret_ref: env/OCEANS_MCP_DISCOVERY_GITHUB_VERIFY` |
| Exa | `https://mcp.exa.ai/mcp?tools=web_search_exa` | `gateway_static_header` | `header_name: x-api-key`, `secret_ref: env/OCEANS_MCP_DISCOVERY_EXA_VERIFY` |
| Exa invalid-key control | Same Exa endpoint | `gateway_static_header` | `header_name: x-api-key`, `secret_ref: env/OCEANS_MCP_DISCOVERY_EXA_INVALID_VERIFY` |

Set `OCEANS_MCP_DISCOVERY_EXA_INVALID_VERIFY` to an arbitrary invalid value, not a real credential. The control sets `expect_tool_error: true`; it requires discovery to succeed, then an aggregate tool result with `isError: true`, an authentication marker, and an `upstream_error` invocation. A successful invalid-key call does not pass the control.

The JSON file accepts one to four candidates. Each needs a unique URL-safe `key`, a `label`, an HTTPS `server_url`, an `auth_mode`, an `auth_config`, and a reviewed `call` with `name` and object `arguments`. A `gateway_static_header` candidate may add `value_template` (for example `"Bearer {{api_key}}"` with `header_name: Authorization`) or use `query_param` instead of `header_name`. Secret references must use `env/OCEANS_MCP_DISCOVERY_*`; do not put credentials in URLs or JSON values. Each candidate is required unless it explicitly sets `required: false`. At least one candidate must be required; an all-optional configuration is rejected before the browser starts. Report optional failures as gaps.

```bash
export OCEANS_VERIFY_MCP_CANDIDATES_FILE=/absolute/path/to/mcp-candidates.json
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin drive mcp
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin evidence mcp
```

- **Register and discover.** The driver opens `Add server`, fills its labelled fields and `Auth config JSON`, then uses `Refresh <display name>` in `Manage MCP server`. It checks `mcp-server-tools` against `/api/v1/admin/mcp/servers/{id}/tools` and confirms the registry count in `mcp-server-list`.
- **Save and edit tool sets.** It uses `New tool set`, `Select <display name>`, and the discovered tool checkboxes. `Save <display name>` must persist the selected IDs, which must remain checked after reload. It checks the disabled-save tooltip, switches between two independent drafts, cancels and confirms `Remove all tools?`, reloads the saved empty set, and uses `Edit <display name>` to save a description without changing membership.
- **Inspect configuration and layout.** `Connection Info` must show the aggregate endpoint and every client block returned by `/api/v1/admin/mcp/connection-info`. The `Choose a tool set` group must remain visible at 390 pixels without horizontal page overflow.
- **Create caller access.** Through `Create API key`, the driver chooses one owner and one explicit model grant because the current UI requires at least one model. It records the chosen model but never calls it. Before an MCP grant, aggregate search must be empty and a direct tool call must return HTTP 403 with a `policy_denied` invocation. Through `Grant subject`, `Grant target`, and `Save grant`, it grants only the saved tool set and checks exact effective tool IDs.
- **Call tools and inspect records.** For each positive candidate, direct `tools/list` must expose only its selected tool and its call must succeed. Aggregate `tools/list` must expose only `search_tools`, `describe_tool`, and `call_tool`; the driver searches within the server, describes the granted address, then calls it with the returned schema hash. Each call must have a matching invocation. The driver applies `mcp-filter-request-id`, checks `mcp-invocations-table`, and opens `Inspect` to match the detail IDs.
- **Retain proof and clean up.** A `finally` block revokes the owned grant and API key, requires HTTP 401 on aggregate and direct routes, and disables the owned tool sets and servers through production admin APIs. It confirms the disabled states and retains the records. `mcp-proof.json` records one check per candidate plus workbench, grant, tool-call, and `cleanup` checks. A candidate whose `OCEANS_MCP_DISCOVERY_*` alias was not set for the launched gateway becomes `blocked` without registration; a blocked required candidate keeps the verdict from `pass`. `evidence mcp` exits non-zero unless the verdict is `pass`. Run stack `cleanup`, then `evidence mcp` again.

Run the local verifier regression tests without upstream calls:

```bash
mise exec -- node --test .agents/skills/verify-oceans-admin/scripts/mcp-verification.test.mjs
```

## Gotchas

- GitHub and Exa use gateway-managed static credentials in this recipe. Passing their calls does not prove OAuth consent, principal-bound credentials, token refresh, or revocation at the upstream service.
- Exa discovery can succeed without a valid key. A catalog result alone is not authentication proof; keep the successful tool call and invalid-key control together.
- Expanded tool rows in `mcp-server-tools` show the upstream `Title` and `Output schema` (`mcp-tool-output-schema-code`) when declared. An `Upstream hints` entry holds the badges in `mcp-tool-annotations`, with the labels `Read-only`, `Destructive` or `Non-destructive`, `Idempotent`, and `Open world` or `Closed world`. The driver does not assert them, because live candidates may not declare annotations. Discovery storage, `describe_tool`/`search_tools` output, and direct `tools/list` passthrough are proved by `mcp_gateway::tests::proxy_policy` and `mcp_gateway::aggregate::tests`.
- The driver calls only `tools/*`. Direct-route governance for `resources/*`, `prompts/*`, `completion/complete`, and unknown methods is proved by the `mcp_gateway::tests::proxy_policy` tests, which run against a mock upstream: a request is denied without a server grant, forwarded with one, has its read results guarded, and is logged with its method.
- `Discover OAuth` is not driven, because it needs an OAuth-protected upstream. With the local stack, run it against a public OAuth-protected server such as `https://mcp.linear.app/mcp` or `https://mcp.notion.com/mcp` and expect `Dynamic client registration`. Without `mcp.oauth.public_base_url` the result also warns that users cannot connect yet. Discovery stores nothing, and the run never completes consent. Parsing, the RFC 9728, RFC 8414, and OpenID Connect fallbacks, and dynamic client registration are proved by `gateway_service::mcp_oauth_discovery::tests`. The form wiring is proved by `mcp-servers-route.test.tsx`.
- Not automated: `Browse catalog` import, credential bindings and `OAuth on-behalf-of`, the `Add to toolset` handoff, the `Preview access` effective-access preview, and UI grant `Revoke`. Cleanup uses `DELETE /api/v1/admin/mcp/grants`.
- Discovery proves schema availability. The generated configuration proves rendered content. This driver does not install or run the listed client applications.
- The temporary API key has one model grant and selected MCP access. It is not an MCP-only key, and this test makes no model-provider call.
- Use a fresh run ID and the stack owned by that run. A failed driver must finish its resource cleanup before stack teardown; inspect cleanup failures before closing the local gateway.
- Proof JSON contains sanitized metadata. The driver dismisses the one-time API key before capture and does not capture invocation detail payloads. The gateway can still retain tool payloads under its normal request-logging policy.
- Session deletion is attempted for sessions issued to the driver. An upstream HTTP 405 is recorded as unsupported termination, not confirmed deletion. Gateway-created upstream sessions are outside this cleanup proof.
- Endpoint URLs and tool schemas can change. Review current upstream documentation and the chosen tool arguments before changing candidates. Upstream outages or missing credentials are reported gaps, not evidence of a passed path.
