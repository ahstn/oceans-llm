# Admin Model Options Listing for Selectors

Status: Accepted

## Context

Three admin UI selectors offer every gateway model: the request log Model filter picker, the agent sessions Model filter, and the spend controls model budget selector. All three called `loadAllModels`, which paged through `GET /api/v1/admin/models` 100 models at a time, with up to four pages in flight.

Each page request ran `AdminModelsService::list_models()` over the whole catalog, then filtered and sliced one page. That build reads effective pricing for every eligible route, matches benchmarks, and renders client configs for each model. A catalog of N×100 models therefore paid for N full builds to fill one dropdown.

The selectors read five fields: `id`, `model_id`, `resolved_model_key`, `alias_of`, and `provider_label`.

## Decision

Add `GET /api/v1/admin/models/options`. It returns every model, aliases included, in one unpaginated response of `AdminModelOptionView` rows carrying those five fields. It keeps the order `list_models` uses.

- `AdminModelsService::list_model_options()` loads models, routes, and route providers in three batched store reads (`load_catalog_routes`). It then picks each model's display route with the same `select_display_route` and `resolve_provider_display` the full summary uses. It skips pricing, allow-lists, benchmarks, and client configs.
- `list_model_items` now starts from the same `load_catalog_routes`, so the two listings cannot drift on alias resolution or the provider label.
- The handler requires an active session, like `list_models`. Every field in the view is already public in the full listing for non-admins, and the view has no route, routing, or allow-list field. So each role gets the same response, and nothing admin-only can leak through it.
- `loadAllModels(signal)` makes one `getModelOptions({ signal })` call and returns `ModelOptionView[]`. The page worker pool, concurrency cap, and abort-on-failure logic are gone. The caller's `AbortSignal` goes to the server function call. Its handler passes the server request's own signal (`currentRequestSignal()`) to the gateway `GET`, so when a request log or agent session filter toolbar unmounts, the gateway request is cancelled too instead of finishing an unused catalog read.

## Alternatives considered

- **Cache the built summary briefly.** This cuts the repeated work but keeps the cost of the full build. It also shows stale models and routes after an admin edit unless every write path invalidates the cache, and it holds a full catalog in memory per gateway instance. The selectors never needed the full summary.
- **Selector search through the existing `q` param.** Every search still builds the whole catalog server-side, so the cost moves to each keystroke instead of going away. The spend controls budget selector is a plain select that lists every model, and the request log picker filters the loaded list on the client.

## Trade-offs

- The response is unpaginated. Each row is a few short strings, so even several thousand models make a small payload. If catalogs grow far past that, add a cursor to this endpoint rather than returning to the summary listing.
- `ModelOptionView` is a second model view in the contract. It is small and derived from the same loader, and a route test checks it against the paginated listing.

## Validation

- `gateway-service`: `model_options_match_summaries_without_reading_pricing` checks that the options make three batched reads and no pricing reads, and match a projection of `list_models`. It covers an alias and a broken alias.
- `gateway` router tests (`http/model_options_tests.rs`): the endpoint returns 401 without a session. For both a member and a platform admin, it returns all 121 seeded models (more than one page) with exactly the five public fields, and it matches the paginated listing row for row.
- Vitest: `model-catalog.test.ts` (one request, signal passed through, rejection), `model-options.server.test.ts` (aborting the browser request aborts the gateway request, through the real server wrapper and gateway fetch), `spend-controls-route.test.tsx` (the loader uses one options request), and the existing request log and agent session Model filter tests, now against `getModelOptions`.

## Follow-up

- `/models` still pages the full summary for its table, which is the listing it needs. Nothing else pages the catalog.
