# Agent Harnesses

Agent Harnesses ranks normalized caller harnesses by request count for a selected observability window and reports their summed input, output, and total tokens without adding token series to the request-count chart.

## Sub-features

- `agent-harnesses.7d-api-parity` — The default 7-day desktop ranking matches the production harness-usage API.
- `agent-harnesses.tokens` — Each ranking row shows request, input-token, output-token, and total-token values, using `n/a` for null token aggregates.
- `agent-harnesses.mastra` — Seeded Mastra traffic is rendered with the Mastra harness icon.
- `agent-harnesses.omp` — Seeded `omp/` traffic is labeled Oh My Pi and rendered with the white `omp.sh` mark.
- `agent-harnesses.31d` — Selecting Last 31 days refreshes the chart and ranking from the 31-day API view.
- `agent-harnesses.responsive` — Mobile cards replace the desktop table below the `md` breakpoint.

## How to get to it (user POV)

- Open `/admin/observability/agent-harnesses` directly. An unauthenticated visit redirects to password sign-in and returns to Agent Harnesses after authentication.
- From another authenticated admin page, open `Observability` in the sidebar and select `Agent Harnesses`.

## Driving it with control-oceans-admin

Preconditions: launch a dedicated verification stack, require `doctor` to pass, and use the seeded `admin@local` platform administrator and refreshed local demo seed.

1. Run `control-oceans-admin drive observability`; after its Leaderboard checks, the driver follows the `Agent Harnesses` sidebar link.
2. Wait for the `Agent harnesses` heading and `harness-usage-table`. The desktop headers must include `Input tokens`, `Output tokens`, and `Total tokens`.
3. Read `/api/v1/admin/observability/harness-usage?range=7d` through the authenticated browser session. Every `harness-usage-table` row must match the API label, key, request count, and formatted nullable token values.
4. Find the API rows keyed `mastra` and `oh_my_pi`. The Mastra table row must contain `[data-agent-harness-icon="mastra"]`; the Oh My Pi row must contain `[data-agent-harness-icon="ohmypi"]` with a `#fff` path fill and no gradient.
5. Require the number of rendered chart areas to match `chart_harnesses`, then capture `04-agent-harnesses-7d.png` and `04-agent-harnesses-7d.aria.txt`.
6. Select the radio named `Last 31 days`. Wait until the rendered harness labels and request counts match `/api/v1/admin/observability/harness-usage?range=31d`, then capture `05-agent-harnesses-31d.png` and `05-agent-harnesses-31d.aria.txt`.
7. Require `observability-proof.json` to retain the API leaders, rendered headers and rows, window boundaries, series counts, Mastra and Oh My Pi icon counts, gateway version, and action log.

## Gotchas

- The route always loads 7 days first; do not infer a 31-day refresh from the selected radio alone. Wait for row values from the 31-day API response.
- `harness-usage-mobile-list` is present but hidden at the driver's desktop viewport. Use `harness-usage-table` for deterministic API comparison and verify responsive behavior in route tests.
- Token sums are ranking-only. The chart remains request-count-only and its values must be compared with `series[].values[].request_count` semantics.
- Oh My Pi deliberately does not reuse the unrelated Pi icon. Its `omp.sh` mark must use a solid white fill, not the source gradient.
- This is a seeded read-only proof. It does not require a paid live LLM request.
