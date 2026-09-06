# MCP live verification, 6 September 2026

The Registry, Tool Sets Workbench, access grants, direct MCP routes, aggregate MCP route, and invocation UI passed against the local gateway and three real upstream services. The check used gateway version `0.30.0` from runtime commit `ad27c631411c842094038b940b5e6906a4b0bf5d`, with the verification drivers added in this change.

## Live service results

| Service | Gateway authentication | Discovered tools | Direct call | Aggregate call |
| --- | --- | ---: | --- | --- |
| Context7 | None | 2 | Passed | Passed |
| GitHub repositories, read-only endpoint | Bearer token | 13 | Passed | Passed |
| Exa search | `x-api-key` header | 1 | Passed | Passed |
| Exa invalid-key control | `x-api-key` header with an invalid value | 1 | Not run | Expected authentication error |

The positive calls used Context7 `resolve-library-id` with a synthetic React query, GitHub `get_file_contents` for the public `octocat/Hello-World` README, and Exa `web_search_exa` with one requested result. Each positive candidate was called once through `/mcp/{server_key}` and once through aggregate `call_tool`. The invalid-key control made one aggregate call. No model-provider request or upstream mutation was sent.

All positive direct results used SSE. Aggregate responses used JSON. The driver required matching JSON-RPC IDs, a nonempty tool result without `isError: true`, and a matching persisted invocation with the expected tool, server, API key, and policy result. Aggregate catalog checks required exactly the three built-ins and exactly one granted tool when filtered to each server.

The Exa control returned an MCP tool error with an authentication marker and a persisted `upstream_error` invocation. Discovery alone did not validate that key. The successful and failed tool calls together distinguish valid Exa credentials from anonymous access.

| Aggregate call | Request ID | Invocation status |
| --- | --- | --- |
| Context7 | `1bebb989-1ab8-4fd8-a7b3-f749c183b05a` | `success` |
| GitHub | `1b4cf0fc-b386-4e33-89db-1b2bd2b738db` | `success` |
| Exa | `cf300ff7-804c-4d55-aa88-218c1fc31fb6` | `success` |
| Exa invalid key | `06229c42-c2f5-4493-96c9-df90d9bb7561` | `upstream_error` |

## Browser and access evidence

The successful MCP run was `20260906-mcp-live-05`. Its browser path covered:

- Registration and refresh through the Registry, with tool rows and counts checked against the production admin API.
- Saved membership after reload, independent drafts in two sets, and metadata edits that preserved membership.
- Cancel and confirm actions for an empty replacement, followed by reload and an empty saved selection.
- Claude Code and Codex configuration blocks checked against the backend response, plus a 390-pixel Workbench viewport without horizontal page overflow.
- A temporary API key created through the UI, followed by four empty aggregate searches and four HTTP 403 direct-call denials before a grant.
- A tool-set grant created through the UI, with effective access limited to the four reviewed tool IDs.
- Request-filtered invocation rows and detail IDs for each aggregate call, including the expected Exa failure.

The Models run `20260906-mcp-live-02` also passed. It matched all 17 displayed and rendered models with the API and compared all OpenCode, Pi, and Codex configuration blocks with the production response. The earlier driver expected the model name alone; the dialog now includes its provider. The corrected driver retains failure screenshots and checks configuration content.

Evidence remains outside the repository:

- `/tmp/oceans-admin-verification/20260906-mcp-live-05/evidence/mcp-proof.json`
- `/tmp/oceans-admin-verification/20260906-mcp-live-05/evidence/cleanup-audit.json`
- Screenshots and ARIA snapshots beside the MCP proof.
- `/tmp/oceans-admin-verification/20260906-mcp-live-02/evidence/models-proof.json`

Earlier MCP attempts stopped at browser loading or incomplete driver interactions. They made no upstream tool calls. Each attempt completed resource cleanup and stack teardown before the next run.

## Cleanup and proof limits

The temporary gateway key was revoked and returned HTTP 401 on aggregate and direct routes. The final database audit found no active grant, server, or tool set owned by the run, and no unrevoked aggregate session for its key. All five aggregate sessions were revoked. The credential-enabled stack was stopped, and retained artifacts passed a scan for the actual GitHub and Exa credentials and raw gateway-key patterns.

GitHub accepted direct session deletion with HTTP 204. Context7 issued no direct session. Exa returned HTTP 405 for direct session deletion; this is recorded as unsupported termination. The check does not prove deletion of gateway-created upstream sessions.

The temporary gateway key had one explicit model grant because the API-key form requires at least one model. The driver made no model call. This was not an MCP-only key test.

GitHub and Exa used static gateway credentials. OAuth consent, principal-bound credentials, token refresh, negotiated protocol changes, and installed client applications were not exercised. Full release gates remain separate from this bounded verification.

## Reusable verification

The project skill now supports `control-oceans-admin drive mcp` and `evidence mcp`. Its candidate file contains endpoint URLs, environment references, and reviewed synthetic calls. Credentials must be loaded into the gateway process before launch and must not be written to the candidate file. The driver uses only the owned local stack, rejects name collisions, records sanitized failure locations, and cleans up its records on failure.

See [the MCP recipe](../../.agents/skills/verify-oceans-admin/features/mcp.md) and [candidate example](../../.agents/skills/verify-oceans-admin/examples/mcp-candidates.json).

Candidate selection used Exa research and primary sources: [Context7 configuration](https://upstash-context7.mintlify.app/mcp/configuration), [GitHub remote MCP server](https://github.com/github/github-mcp-server/blob/main/docs/remote-server.md), and [Exa MCP documentation](https://exa.ai/docs/reference/exa-mcp). Exa's [request authentication](https://github.com/exa-labs/exa-mcp-server/blob/main/api/mcp.ts) and [search implementation](https://github.com/exa-labs/exa-mcp-server/blob/main/src/tools/webSearch.ts) explain why the negative control must execute a tool.

## Corrections after verification review

The review found several introduced issues outside the successful live-call path. This change also:

- Preserves carried tool IDs through the legacy `tab=toolsets` redirect and recovers from stale selected-set links. A new draft stays selected while loader data catches up.
- Trims tool-set and tool IDs before forwarding validated values to the gateway.
- Publishes current grid context values without render-phase ref writes. Immediate autosize uses the current render; delayed work uses the latest committed table and cancels stale candidate work. Context can now cause more renders during resizing; unchanged resize performance is not claimed.
- Uses `OCEANS_LLM_API_KEY` consistently in manual client examples, removes an unsupported test query option, and installs UI dependencies before both preview tasks.

The final browser check, `20260906-mcp-ui-final-02`, passed Registry search, the 13-row cached GitHub catalog, stale-selection recovery, and legacy tool handoff. It made no upstream calls. Its proof is `/tmp/oceans-admin-verification/20260906-mcp-ui-final-02/evidence/mcp-ui-followup-proof.json`.

Validation after these changes:

- Repository `mise run lint` passed.
- The final client and server UI bundles built successfully with `mise run ui-build`.
- Forty navigation, Workbench, and admin-boundary tests passed. Twenty-two grid and server-route tests passed. Five connection-dialog tests passed.
- Focused formatting, script syntax, task parsing, and `git diff --check` passed.
- Focused React Doctor `0.9.13` reported zero errors and seven warnings for the grid. This is not a clean warning-blocking CI result. The PR workflow blocks on warnings; its earlier full scan reported three errors and 52 warnings.
- Standalone TypeScript checking still reported 194 diagnostics outside the two grid files. A successful project-wide typecheck is not claimed.

The existing OAuth-state consumption and ID-less upstream-error review findings also exist in the base branch. They remain separate follow-up work. No review replies were posted and no review discussions were resolved by this verification.
