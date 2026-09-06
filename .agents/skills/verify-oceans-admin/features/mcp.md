# MCP

MCP verification follows the admin browser path from server registration to tool access. It checks the Tool Sets workbench, generated client configuration, direct and aggregate tool calls, upstream authentication failure, and invocation records against the real local gateway.

## Sub-features

- `mcp-registry` creates servers, refreshes discovery, and compares tool rows and registry counts with the production API.
- `mcp-toolsets` saves membership, checks it after reload, keeps drafts independent between two sets, confirms an empty replacement, and preserves membership during a metadata edit.
- `mcp-client-config` compares each Connection Info client block with the backend response and checks the workbench at a 390-pixel viewport.
- `mcp-grants` proves denial before an explicit tool-set grant and confirms the exact effective tool list after the grant.
- `mcp-routing` calls one reviewed tool through each direct route and through aggregate `search_tools`, `describe_tool`, and `call_tool`.
- `mcp-upstream-auth` compares a valid Exa key with an invalid-key control and requires a recorded authentication error from the control.
- `mcp-invocations` matches calls to request-linked invocation records, then checks the filtered list and detail in the UI.
- `mcp-cleanup` revokes the grant and API key, checks HTTP 401, and disables the servers and tool sets created by the run.

## How to get to it (user POV)

- Open `/admin/mcp` and sign in as the seeded platform administrator. The registry is also available at `/admin/mcp?tab=servers`.
- Open `/admin/mcp/toolsets` for the Tool Sets workbench and its `Connection Info` dialog.
- Open `API Keys` at `/admin/api-keys` to create the temporary caller key, then `/admin/mcp?tab=access` to grant the saved tool set.
- Connect an MCP client to `/mcp/{server_key}` for one server or `/mcp` for the aggregate catalog.
- Open `MCP invocations` under `Observability` at `/admin/observability/mcp-invocations` to inspect a call.

## Driving it with control-oceans-admin

Preconditions:

- Use the owned local stack and require `control-oceans-admin doctor` to pass. The platform administrator needs the `mcp`, `api_keys`, and `mcp_invocations` pages plus `create_api_key` and `revoke_api_key` actions. Root `gateway.yaml` grants these through the configured permission groups.
- Before launch, export `GATEWAY_CLIENT_CONFIG_BASE_URL=http://127.0.0.1:$OCEANS_VERIFY_GATEWAY_PORT`. This value controls generated client configuration; the driver's `expected_gateway_url` must contain the same origin followed by `/mcp`.
- Load the required credential aliases into the gateway process before launch. Setting them only in the driver process cannot change an already running gateway. The example uses the GitHub token from `gh auth token --hostname github.com` and Exa's `EXA_API_KEY` from Mise. Keep their values in process memory and out of the candidate file and evidence.
- Use only reviewed read-only tools with public or synthetic arguments. The example sends two successful upstream tool calls per positive candidate, plus one failing aggregate call for the invalid-key control. Discovery and protocol setup add requests. MCP calls can consume service quota; no LLM provider request is sent.

The [candidate example](../examples/mcp-candidates.json) uses these endpoint and authentication settings:

| Candidate | HTTPS endpoint | Auth mode | Auth config |
| --- | --- | --- | --- |
| Context7 | `https://mcp.context7.com/mcp` | `none` | `{}` |
| GitHub | `https://api.githubcopilot.com/mcp/x/repos/readonly` | `gateway_bearer_token` | `secret_ref: env/OCEANS_MCP_DISCOVERY_GITHUB_VERIFY` |
| Exa | `https://mcp.exa.ai/mcp?tools=web_search_exa` | `gateway_static_header` | `header_name: x-api-key`, `secret_ref: env/OCEANS_MCP_DISCOVERY_EXA_VERIFY` |
| Exa invalid-key control | Same Exa endpoint | `gateway_static_header` | `header_name: x-api-key`, `secret_ref: env/OCEANS_MCP_DISCOVERY_EXA_INVALID_VERIFY` |

Set `OCEANS_MCP_DISCOVERY_EXA_INVALID_VERIFY` to an arbitrary invalid value, not a real credential. The control sets `expect_tool_error: true`; it requires discovery to succeed, then an aggregate tool result with `isError: true`, an authentication marker, and an `upstream_error` invocation. A successful invalid-key call does not pass the control.

The JSON file accepts one to four candidates. Each needs a unique URL-safe `key`, a `label`, an HTTPS `server_url`, an `auth_mode`, an `auth_config`, and a reviewed `call` with `name` and object `arguments`. Secret references must use `env/OCEANS_MCP_DISCOVERY_*`; do not put credentials in URLs or JSON values. Each candidate is required unless it explicitly sets `required: false`. At least one candidate must be required; an all-optional configuration is rejected before the browser starts. Report optional failures as gaps.

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
- **Retain proof and clean up.** A `finally` block revokes the owned grant and API key, requires HTTP 401 on aggregate and direct routes, and disables the owned tool sets and servers through production admin APIs. It confirms the disabled states and retains the records. Require `mcp-proof.json` to show `passed: true` and `cleanup.passed: true`; the `evidence mcp` command checks file presence only. Run stack `cleanup`, then `evidence mcp` again.

Run the local verifier regression tests without upstream calls:

```bash
mise exec -- node --test .agents/skills/verify-oceans-admin/scripts/mcp-verification.test.mjs
```

## Gotchas

- GitHub and Exa use gateway-managed static credentials in this recipe. Passing their calls does not prove OAuth consent, principal-bound credentials, token refresh, or revocation at the upstream service.
- Exa discovery can succeed without a valid key. A catalog result alone is not authentication proof; keep the successful tool call and invalid-key control together.
- Discovery proves schema availability. The generated configuration proves rendered content. This driver does not install or run the listed client applications.
- The temporary API key has one model grant and selected MCP access. It is not an MCP-only key, and this test makes no model-provider call.
- Use a fresh run ID and the stack owned by that run. A failed driver must finish its resource cleanup before stack teardown; inspect cleanup failures before closing the local gateway.
- Proof JSON contains sanitized metadata. The driver dismisses the one-time API key before capture and does not capture invocation detail payloads. The gateway can still retain tool payloads under its normal request-logging policy.
- Session deletion is attempted for sessions issued to the driver. An upstream HTTP 405 is recorded as unsupported termination, not confirmed deletion. Gateway-created upstream sessions are outside this cleanup proof.
- Endpoint URLs and tool schemas can change. Review current upstream documentation and the chosen tool arguments before changing candidates. Upstream outages or missing credentials are reported gaps, not evidence of a passed path.
