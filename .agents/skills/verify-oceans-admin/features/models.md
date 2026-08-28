# Models

Models lets a signed-in user review configured model IDs, routing status, provider details, pricing, and generated client configuration choices. Platform administrators can also review access rules and refresh pricing.

## Sub-features

- `models-open` opens Models from the Control Plane sidebar.
- `models-list` shows the configured model count and desktop or mobile list.
- `models-info` opens routing, economics, and access details for one model.
- `models-columns` toggles context-window and capability columns.
- `models-client-config` opens client configuration for a configurable model.

## How to get to it (user POV)

- Sign in and choose `Models` under `Control Plane` in the sidebar.
- Open `/admin/models`; an unauthenticated user is sent to `/admin/login` first.

## Driving it with control-oceans-admin

Preconditions:

- The recorded stack is healthy and contains the demo models from `gateway.yaml`.
- The browser viewport is at least 768 pixels wide for the desktop table.
- `control-oceans-admin doctor` passes.

- **Automated proof.** Run `control-oceans-admin drive models`. It signs in, follows the `Models` link, compares `Showing N of N models` with `/api/v1/admin/models?page=1&page_size=100`, checks all four platform-admin information sections, enables the optional columns, and opens client configuration for `gpt-5.6-sol`.
- **Known model.** Locate `models-desktop-cell-gpt-5.6-sol`. The cell shows `gpt-5.6-sol` and a status indicator.
- **Model detail.** In the same table row, choose `Info`. A dialog headed `Model info` contains `gpt-5.6-sol` and navigation named `Model info sections`.
- **Columns.** Choose `Columns`, then select `Context window` or `Capabilities`. The matching table header becomes visible without a route change.
- **Client configuration.** Choose the button named `Generate client config for gpt-5.6-sol`. A dialog headed `Client config` appears. Do not treat generated configuration as live-provider proof.
- **Proof.** Retain the five screenshots, five ARIA snapshots, and `models-proof.json` in the run evidence directory.

## Gotchas

- The mobile list replaces `models-desktop-table` below the `md` breakpoint.
- Startup seeds configured providers without checking external credentials. A healthy Models page does not prove upstream access.
- `Refresh pricing` can call the pricing refresh boundary and is outside the read-only baseline.
- Some aliases have no independent client configuration. Use `gpt-5.6-sol` for the baseline detail proof.
