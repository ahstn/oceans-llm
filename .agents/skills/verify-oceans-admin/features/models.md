# Models

Models lets a signed-in user review configured model IDs, routing status, provider details, pricing, sourced benchmark scores, capabilities, and generated client configuration choices, 30 models per page. Platform administrators also see the `Allow List` column and access rules, and can refresh catalogues.

## Sub-features

- `models-open` opens Models from the Control Plane sidebar.
- `models-list` shows the configured model count and desktop or mobile list, including the always-visible Intelligence Index.
- `models-info` opens overview, routing, economics, benchmarks, and (for platform admins) access details for one model.
- `models-columns` toggles the optional context-window and capability columns.
- `models-client-config` opens client configuration for a configurable model via the row `Config` button.
- `models-select-config` selects several configurable models with `Select model <id>` and opens one `Client config` dialog through `Generate config`.
- `models-pagination` pages 30 models at a time with `Previous` and `Next`.
- `models-litellm-info` serves the LiteLLM-compatible `GET /model_group/info` list to gateway API keys, so clients such as omp can use `discovery.type: litellm`.

## How to get to it (user POV)

- Sign in and choose `Models` under `Control Plane` in the sidebar.
- Open `/admin/models`; an unauthenticated user is sent to `/admin/login` first.

## Driving it with control-oceans-admin

Preconditions:

- Sign in as the seeded `admin@local` platform administrator so the `Access` model information section is available.
- The recorded stack is healthy and contains the `gpt-6-astra` demo model from `gateway.yaml`.
- The browser viewport is at least 768 pixels wide for the desktop table.
- `control-oceans-admin doctor` passes.

- **Automated proof.** Run `control-oceans-admin drive models`. It ensures Playwright Chromium is installed, signs in, follows the `Models` link, reads the `Model pagination` range (`first–last of total`, next to `Rows per page`), checks the displayed range size against rendered rows and the total against `/api/v1/admin/models?page=1&page_size=100&include_aliases=false` (the same direct-model view the page loads), checks the always-visible Intelligence Index against that API response, checks all four platform-admin information sections (`Overview`, including `Pricing and limits`, `Routing`, `Benchmarks`, and `Access`), compares the benchmark state with that API response, checks the Artificial Analysis attribution below the model list, enables the optional Context window and Capabilities columns, and opens client configuration for `gpt-6-astra`.
- **Known model.** Locate `models-desktop-cell-gpt-6-astra`. The cell shows `gpt-6-astra` and a status indicator.
- **Model detail.** In the same table row, choose `Info`. A dialog headed `Model info` contains `gpt-6-astra` and navigation named `Model info sections`.
- **Columns.** Intelligence Index is always visible. Choose `Columns`, then select `Context window` or `Capabilities`. The matching optional table header becomes visible without a route change.
- **Client configuration.** Choose the button named `Generate client config for gpt-6-astra`. A dialog headed `Client config` appears. Do not treat generated configuration as live-provider proof.
- **Decisions-only model.** The `jev` row shows `—` in its Config cell, and `Select model jev` is disabled.
- **LiteLLM model info.** Create a temporary user API key through [API keys](./api-keys.md) and keep it in memory. `GET /model_group/info` with `Authorization: Bearer <key>` returns `200`. The set of `data[].model_group` IDs equals the set of `data[].id` from `GET /v1/models` with the same key. Every row has a non-empty lowercase `providers` array, `mode` (`chat` or `embedding`), a boolean `supports_vision`, a boolean `supports_function_calling`, and a `supported_openai_params` array. A priced row has `input_cost_per_token` and `output_cost_per_token`, plus `cache_read_input_token_cost` and `cache_creation_input_token_cost` when the cache prices are known; an unpriced row has none of them. The response must not contain a provider base URL or credential. This check makes no provider call. `drive models` does not run it, so report it beside the driver proof. Revoke the key afterwards.
- **Proof.** Retain the Models screenshots and ARIA snapshots, including `03-model-benchmarks` (plus one `05-model-client-config-<key>` pair per client config), and `models-proof.json` in the run evidence directory.

## Gotchas

- The mobile list replaces `models-desktop-table` below the `md` breakpoint.
- Startup requires `env.*` credential references to be configured and present, but it does not validate credential correctness or upstream connectivity. A healthy Models page does not prove upstream access.
- `Refresh pricing` can call the pricing refresh boundary and is outside the read-only baseline.
- Some aliases and decisions-only models such as `jev` have no client configuration. Use `gpt-6-astra` for the baseline detail proof.
- Demo model IDs follow `gateway.yaml`. When a proof times out waiting for a model cell, check whether the model was renamed there before suspecting the UI.
