# Models

Models lets a signed-in user review configured model IDs, routing status, provider details, pricing, capabilities, and generated client configuration choices, 30 models per page. Platform administrators also see the `Allow List` column and access rules, and can refresh pricing.

## Sub-features

- `models-open` opens Models from the Control Plane sidebar.
- `models-list` shows the configured model count and desktop or mobile list.
- `models-info` opens overview, routing, economics, and (for platform admins) access details for one model.
- `models-columns` toggles context-window and capability columns.
- `models-client-config` opens client configuration for a configurable model via the row `Config` button.
- `models-select-config` selects several configurable models with `Select model <id>` and opens one `Client config` dialog through `Generate config`.
- `models-pagination` pages 30 models at a time with `Previous` and `Next`.

## How to get to it (user POV)

- Sign in and choose `Models` under `Control Plane` in the sidebar.
- Open `/admin/models`; an unauthenticated user is sent to `/admin/login` first.

## Driving it with control-oceans-admin

Preconditions:

- Sign in as the seeded `admin@local` platform administrator so the `Access` model information section is available.
- The recorded stack is healthy and contains the `gpt-6-astra` demo model from `gateway.yaml`.
- The browser viewport is at least 768 pixels wide for the desktop table.
- `control-oceans-admin doctor` passes.

- **Automated proof.** Run `control-oceans-admin drive models`. It ensures Playwright Chromium is installed, signs in, follows the `Models` link, checks the displayed count against rendered rows and the total count against `/api/v1/admin/models?page=1&page_size=100`, checks all four platform-admin information sections, enables the optional columns, and opens client configuration for `gpt-6-astra`.
- **Known model.** Locate `models-desktop-cell-gpt-6-astra`. The cell shows `gpt-6-astra` and a status indicator.
- **Model detail.** In the same table row, choose `Info`. A dialog headed `Model info` contains `gpt-6-astra` and navigation named `Model info sections`.
- **Columns.** Choose `Columns`, then select `Context window` or `Capabilities`. The matching table header becomes visible without a route change.
- **Client configuration.** Choose the button named `Generate client config for gpt-6-astra`. A dialog headed `Client config` appears. Do not treat generated configuration as live-provider proof.
- **Decisions-only model.** The `jev` row shows `—` in its Config cell, and `Select model jev` is disabled.
- **Proof.** Retain at least five screenshots and ARIA snapshots (plus one `05-model-client-config-<key>` pair per client config) and `models-proof.json` in the run evidence directory.

## Gotchas

- The mobile list replaces `models-desktop-table` below the `md` breakpoint.
- Startup requires `env.*` credential references to be configured and present, but it does not validate credential correctness or upstream connectivity. A healthy Models page does not prove upstream access.
- `Refresh pricing` can call the pricing refresh boundary and is outside the read-only baseline.
- Some aliases and decisions-only models such as `jev` have no client configuration. Use `gpt-6-astra` for the baseline detail proof.
- Demo model IDs follow `gateway.yaml`. When a proof times out waiting for a model cell, check whether the model was renamed there before suspecting the UI.
