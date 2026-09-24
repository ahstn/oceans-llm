# Models

Models lets a signed-in user review configured model IDs, routing status, provider details, pricing, sourced benchmark scores, and generated client configuration choices. Platform administrators can also review access rules and refresh catalogues.

## Sub-features

- `models-open` opens Models from the Control Plane sidebar.
- `models-list` shows the configured model count and desktop or mobile list.
- `models-info` opens routing, economics, benchmarks, and access details for one model.
- `models-columns` toggles context-window, capability, and Intelligence columns.
- `models-client-config` opens client configuration for a configurable model.

## How to get to it (user POV)

- Sign in and choose `Models` under `Control Plane` in the sidebar.
- Open `/admin/models`; an unauthenticated user is sent to `/admin/login` first.

## Driving it with control-oceans-admin

Preconditions:

- Sign in as the seeded `admin@local` platform administrator so the `Access` model information section is available.
- The recorded stack is healthy and contains the `gpt-6-astra` demo model from `gateway.yaml`.
- The browser viewport is at least 768 pixels wide for the desktop table.
- `control-oceans-admin doctor` passes.

- **Automated proof.** Run `control-oceans-admin drive models`. It ensures Playwright Chromium is installed, signs in, follows the `Models` link, checks the displayed count against rendered rows and the total count against `/api/v1/admin/models?page=1&page_size=100`, checks all five platform-admin information sections, compares the benchmark state with that API response, checks the Artificial Analysis attribution below the model list, enables the optional columns, and opens client configuration for `gpt-6-astra`.
- **Known model.** Locate `models-desktop-cell-gpt-6-astra`. The cell shows `gpt-6-astra` and a status indicator.
- **Model detail.** In the same table row, choose `Info`. A dialog headed `Model info` contains `gpt-6-astra` and navigation named `Model info sections`.
- **Columns.** Choose `Columns`, then select `Context window`, `Capabilities`, or `Intelligence`. The matching table header becomes visible without a route change.
- **Client configuration.** Choose the button named `Generate client config for gpt-6-astra`. A dialog headed `Client config` appears. Do not treat generated configuration as live-provider proof.
- **Proof.** Retain the Models screenshots and ARIA snapshots, including `03-model-benchmarks`, plus `models-proof.json` in the run evidence directory.

## Gotchas

- The mobile list replaces `models-desktop-table` below the `md` breakpoint.
- Startup requires `env.*` credential references to be configured and present, but it does not validate credential correctness or upstream connectivity. A healthy Models page does not prove upstream access.
- `Refresh pricing` can call the pricing refresh boundary and is outside the read-only baseline.
- Some aliases have no independent client configuration. Use `gpt-6-astra` for the baseline detail proof.
