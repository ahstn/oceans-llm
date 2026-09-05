# MCP design candidates

Run `mise run ui-install`, then `mise run ui-designs` from the repository root. Open [the comparison page](http://127.0.0.1:4317/designs/index.html). Use the candidate selector to compare the same data in each layout. The sun/moon button switches the theme.

These are interactive replacement candidates with synthetic data. They are a separate Vite entry, without gateway calls or authentication changes. Edits remain in browser memory. Servers candidate A is now applied to the production Registry. Tool Sets uses the selected Workbench design at `/mcp/toolsets`. The Workbench preview uses the same presentation component as the live page; other candidates remain available as design references.

## Findings from the current pages

[`servers.tsx`](../src/routes/mcp/servers.tsx) redirects to `/mcp?tab=servers`, preserving the server ID. The active view is [`-servers-tab.tsx`](../src/routes/mcp/-servers-tab.tsx).

The previous table put endpoint URLs and repeated refresh/edit/disable actions ahead of discovery information. It showed registration status, but tool counts, discovery failures, and the last successful discovery required opening a dialog. It had no server search or filter. The previous recommended catalog had less context than the available descriptions and authentication fields support.

The candidates retain the useful patterns from Models, Teams, API Keys, and Account Connections: quiet page backgrounds, clear primary actions, compact secondary identifiers, outlined tables, responsive cards, and contextual management. Overview, Configuration, Tools, and Credentials remain inside the management dialog, as required by the admin UI conventions.

## Server candidate comparison

| Candidate                                                                       | Main design change                                                                                                                      | Best use                                        | Trade-off                                                      |
| ------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------- | -------------------------------------------------------------- |
| [A — Registry](http://127.0.0.1:4317/designs/index.html?candidate=registry)     | Searchable, sortable ReUI data grid with discovery result, time, tool count, and registration shown together. Mobile uses compact rows. | Daily administration and larger server lists.   | Less room for descriptions.                                    |
| [B — Library](http://127.0.0.1:4317/designs/index.html?candidate=library)       | Service cards expose descriptions, authentication, saved tool counts, and management actions. A catalog prompt supports setup.          | Browsing and smaller installations.             | More vertical space and slower comparison across many servers. |
| [C — Operations](http://127.0.0.1:4317/designs/index.html?candidate=operations) | A server list sits beside a ReUI Frame with the selected discovery result, last success, and recovery actions. Failures appear first.   | Investigating discovery or credential problems. | More information is visible at once.                           |

**Selected: A.** It fits the established admin pages and improves the most frequent task: finding a server and understanding its state. B is the strongest alternative when users browse services more often than they investigate failures. C makes the operational workflow explicit.

## Tool Sets navigation and candidates

Tool Sets now has a dedicated route at `/mcp/toolsets`. Sibling links keep Servers, Tool Sets, and Access together. Old `/mcp?tab=toolsets` links redirect and preserve the selected set. Selecting tools in Servers transfers the tool IDs to the new page. The page consumes those IDs into a pending selection, removes them from the URL, and merges them with saved members when the first destination is selected or created. Each set keeps its own draft when the user switches targets. Access remains the separate grant-management flow.

The existing Tool Sets page combined a narrow master list, inline metadata form, and popover tool picker. This made the tool catalog harder to inspect and the full replacement action easy to miss. The three candidates use the same metadata, tool catalog, and replacement semantics:

| Candidate                                                                          | Approach                                                                                                                                                        | Trade-off                                                        |
| ---------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| [A — Directory](http://127.0.0.1:4317/designs/toolsets.html?candidate=directory)   | A sortable ReUI directory opens a focused editor with tool selection and draft review.                                                                          | Closest to Registry; editing requires opening a dialog.          |
| [B — Workbench](http://127.0.0.1:4317/designs/toolsets.html?candidate=workbench)   | The left navigator contains each set’s tool count, status, Edit, and Save. The grouped tool catalog uses the remaining width; there is no separate draft panel. | Fast for repeated curation; uses more screen space.              |
| [C — Guided Builder](http://127.0.0.1:4317/designs/toolsets.html?candidate=guided) | Choose a set, choose tools, then review the replacement in three steps.                                                                                         | Clear sequence for occasional use; more steps for routine edits. |

**Final design: B — Workbench, with navigator controls.** The production page at `/mcp/toolsets` uses the shared `ToolsetWorkbench` and `ToolsetConnectionDialog` components. The preview defaults to Workbench; A and C remain design references. The key row was removed from the navigator and replaced by the tool count plus Edit and Save buttons. Counts update with the draft; Unsaved marks pending changes. A current set outside a search or status filter stays visible, so its Save control remains available. The selected set’s tools occupy the second column.

The shared Workbench places its filters, navigator, and tool catalog inside the standard admin Card. Hover and selection highlight the full navigator item, including its count and action area. Active badges use the existing success green, and tool counts align with the icons above. Edit and enabled Save have distinct hover states; Save stays disabled until there are changes to save.

Connection Info sits beside Manage access. Its horizontal configuration selector matches the Models client-config dialog, with full-width content beneath it and no sidebar. In production, it loads `/api/v1/admin/mcp/connection-info`; the handler calls `gateway-client-config::render_mcp_client_configs` to generate Claude Code JSON and Codex TOML. The shared `/mcp` endpoint uses the same `GATEWAY_CLIENT_CONFIG_BASE_URL` as Models. Tool sets are access bundles, so the dialog explains that the API key's grants determine its tools. The isolated preview injects a synthetic `McpConnectionInfoPayload` with `gateway.example.com` and makes no gateway calls.

### Tool Sets data boundary

`McpToolsetView` provides metadata and status. A new authenticated GET on `/api/v1/admin/mcp/toolsets/{toolset_id}/tools` returns saved member IDs through the existing `McpToolsetToolsPayload`. It distinguishes a known empty set from an unknown set and supports both LibSQL and PostgreSQL. The navigator displays a real count after this read, and the editor starts with saved selections. Loading and failed reads never appear as zero tools.

Drafts and saved snapshots stay separate for each set. Row Save captures that set’s ID and selection, so changing the selected workspace cannot redirect an in-flight save. Failures keep the draft. A previously populated set requires confirmation before it is saved empty. Saved tools that are inactive or belong to an unavailable server remain visible until the user removes them; they are not silently dropped. Disabled tool sets remain editable, as the API permits.

The previews use synthetic saved memberships. Their controls cover metadata edits, per-set saving, imported tools, catalog scenarios, schemas, and unavailable tools. The Access control explains the separate grant flow and does not simulate access changes.

## Data and interaction boundaries

All candidates use `McpServerView`. Registration and discovery are separate concepts. A server can be active while discovery requires authentication. Missing tool counts display as unknown, rather than zero. No live health, uptime, latency, or invocation metrics are invented.

The preview supports search, filters, empty results, selection, theme changes, configuration edits, adding sample servers, catalog templates, and collapsed illustrative tool schemas. Refresh shows a pending state and returns the fixed sample result. Credential binding changes and real toolset creation belong to the existing production dialog; they are outside this design comparison.

The current gateway status names are used: `success`, `failed`, `auth_required`, and `disabled`. Authentication labels map to the current gateway modes. Tool schemas are explicitly marked as illustrative.

## Component sources

The implementation uses the project’s Radix Nova shadcn components and Hugeicons. Brand marks use the existing Lobe SVG package with quoted CSS mask URLs for light and dark themes.

- [shadcn Card](https://ui.shadcn.com/docs/components/radix/card), [Dialog](https://ui.shadcn.com/docs/components/radix/dialog), and [Input Group](https://ui.shadcn.com/docs/components/radix/input-group).
- ReUI `@reui/data-grid`, based on its registry example `c-data-grid-1`; [Data Grid preview and API](https://reui.io/docs/data-grid).
- ReUI `@reui/frame`; [Frame preview and API](https://reui.io/docs/frame).
- Existing ReUI `IconTile`; [Icon Tile preview and API](https://reui.io/docs/icon-tile).

The ReUI MCP tools were not available in the session. Components and examples were inspected with the configured `@reui` registry through the shadcn CLI. Installed source uses TanStack Table 9, so the implementation follows its actual `useTable` API. Unused drag, resize UI, virtualization, filtering plugins, and duplicate badge source were removed. The retained upstream grid renderer remains large; its local lint exceptions explain why the source was preserved.

## Verification

- `mise run ui-check`: formatting, lint, 215 tests, and client/SSR builds passed. Existing complexity and bundle-size warnings remain.
- `mise run ui-designs-build`: isolated preview build passed.
- With the preview running, `mise exec -- bun run --cwd crates/admin-ui/web designs/verify.mjs` checks the three layouts in Chromium and writes screenshots under `test-results/mcp-designs`. Set `MCP_DESIGN_SCREENSHOTS` to change that directory.
- `mise exec -- bun run --cwd crates/admin-ui/web designs/verify-workbench.mjs` checks the selected Workbench, including counts from synthetic saved memberships, navigator Edit/Save, per-set drafts, empty clear, handoff merge, catalog failures, unavailable tools, schema containment, filtered selection, and keyboard controls. The script writes proof and screenshots under `test-results/toolsets-revised` by default; set `TOOLSET_DESIGN_SCREENSHOTS` to override the output directory.
- Standalone `tsc --noEmit` still reports errors in existing routes, AppIcon, and chart types. It reports no diagnostics in the preview or new presentation files. The new Tool Sets loader and existing metadata CRUD functions encounter the same pre-existing server-function input typing issues. The membership GET/Save functions use typed runtime validators and produce no new diagnostics.
- Manual browser checks on an owned local gateway/UI stack passed login, Registry/API parity, search/catalog, server metadata creation and edits, Tool Sets creation and edits, selected-ID legacy redirects, Access navigation, and 390px containment. Temporary records were disabled and confirmed through the API. No provider discovery was triggered.
- Live testing found a new Registry date-format hydration mismatch, which was fixed with deterministic UTC output and a regression test. Root Toaster hydration warnings also occur on the unchanged Models page. Route Suspense hydration warnings remain on both the Tool Sets redirect and the unchanged legacy Servers redirect; the tested flows complete successfully.

- `mise run lint` passed for the mixed Rust/UI change. The new membership endpoint has four handler tests, one OpenAPI test, and a PostgreSQL regression run against a disposable live database.
- A second owned-stack run, `20260905-workbench-membership-qa`, verified the final Workbench through the real gateway. A loopback MCP fixture supplied three harmless tool schemas through initialize and tools/list. Browser checks covered discovery, navigator counts, Edit, Save, saved preselection in a fresh browser, separate drafts, saving an unselected set, empty-clear confirmation, and 390px containment. API reads confirmed the saved IDs. Temporary sets/server were disabled and both stack and fixture were stopped.

The candidate comparisons use sample data. The final local proof exercises real membership reads and writes through the gateway with a loopback MCP fixture. It makes no paid calls, external provider requests, or tool executions.
