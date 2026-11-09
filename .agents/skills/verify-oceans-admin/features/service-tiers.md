# Service tiers

Service tier verification proves that callers can ask for a provider capacity tier (`default`, `flex`, `priority` or its alias `fast`, `ultrafast`), that the gateway sends it to the upstream provider, and that it records the requested and served tier. It also proves the tier policy and routing rejections. The design is in [the service tiers ADR](../../../../docs/adr/2026-11-09-service-tiers.md).

## Sub-features

- `tier-discovery` confirms that `/v1/models` lists `service_tiers` for each model and that `/model_group/info` lists `service_tier` in `supported_openai_params`.
- `tier-negative` confirms the free rejections: an unknown value (400), `service_tier` and `speed` that ask for different tiers (400), a tier outside the model's `service_tiers` allow-list (403 `service_tier_not_allowed`), and a tier that no route of the model can serve (400).
- `tier-ab` sends the same prompt through `gpt-6-luna` at the default, priority (`service_tier: "fast"`) and flex tiers. The upstream prompt cost per token must be about 2× and 0.5× of default.
- `tier-forced-route` sends `service_tier: "default"` to `gpt-6-luna-fast`. The route forces priority, so the provider serves priority.
- `tier-messages-downgrade` sends Anthropic `speed: "fast"` to `claude-haiku-5.5`. Haiku has no fast mode, so the provider serves standard. The request log shows requested `priority` and served `default`.
- `tier-stream` streams a priority request and confirms that the stream and the request log carry the served tier.
- `tier-request-log` confirms `metadata.service_tier` (requested) and `metadata.served_service_tier` for each paid request, and that the request detail dialog shows a `Service tier` fact.
- `tier-key-cleanup` revokes the temporary key and confirms that gateway authentication rejects it.

## How to get to it (user POV)

- Sign in as the seeded platform administrator. Open `API Keys` and create a temporary key with access to `gpt-6-luna`, `gpt-6-luna-fast`, `claude-haiku-5.5`, `deepseek-v4.1-flash` and `claude-sonnet`.
- Call `/v1/chat/completions` or `/v1/responses` with `service_tier`, or `/v1/messages` with `speed: "fast"`.
- Open `Request Logs` under `Observability`, filter by the request ID, and choose `Inspect`. The `Service tier` fact shows the requested and served tier.
- Open `Models` and the model info dialog to see the allowed tiers, the route's forced tier and the tier prices.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes for the stack owned by this verification run.
- `OPENROUTER_API_KEY` resolves through fnox, which hands it to `dev-stack` (see Launch in [SKILL.md](../SKILL.md)). The harness never reads, prints, or saves it.
- Root `gateway.yaml` defines `gpt-6-luna` and `gpt-6-luna-fast` (route `service_tier: priority`) on the `openrouter` provider, and `deepseek-v4.1-flash` with `service_tiers: [flex]`.
- The operator accepts six short paid requests. Five go to `openai/gpt-6-luna` and one to `anthropic/claude-haiku-5.5`. Each request sets `max_tokens` to 256 or less. At the prices in Gotchas the total is below $0.001.

- **Automated proof.** Run `control-oceans-admin drive service-tiers`. The driver signs in through the real browser route and creates one user-owned key; the raw key stays only in process memory.
- **Discovery.** `gpt-6-luna` must list `priority` and `flex`, `gpt-6-luna-fast` must list `priority`, and `deepseek-v4.1-flash` must not list `priority`.
- **Negative checks.** These are free: the gateway rejects each request before any provider call.
- **A/B checks.** The driver reads `usage.cost_details.upstream_inference_prompt_cost` from OpenRouter's response and divides it by the prompt tokens. Priority must be 1.8–2.2× default, and flex 0.4–0.6×.
- **Request-log proof.** For each paid request, the driver loads the request log by request ID and checks the requested and served tier in `log.metadata`. It opens the detail dialog for the priority and the downgraded request, and captures screenshots with `request-log-payloads` masked.
- **Cleanup.** The driver revokes the temporary key in a `finally` block and confirms that `/v1/models` rejects it. It writes only sanitized metadata to `service-tiers-proof.json`.
- **Proof.** Run `control-oceans-admin evidence service-tiers` before and after stack cleanup.

## Gotchas

- `cost_usd_10000` in the request log has four decimal places. A canary request costs much less than $0.0001, so the gateway cost is `0` or `1` for every tier. The A/B check reads OpenRouter's cost split instead. The offline tests in `gateway-service` prove that catalog prices use the tier rates.
- On a BYOK OpenRouter account, `usage.cost` is `0` and `cost_details.upstream_inference_cost` holds the provider's charge. The gateway adds them. The cost split is present for both BYOK and credit requests.
- OpenRouter prices for `openai/gpt-6-luna` when this was written, in USD per million tokens: default 0.10 input and 0.50 output, priority (`openai/fast` endpoint) 0.20 and 1.00, flex 0.05 and 0.25.
- OpenAI can serve a priority request at the default tier when it is short of capacity, and then reports `service_tier: "default"`. The driver then fails with a served-tier mismatch. This is a provider decision, not a gateway fault. Retry once after a few minutes.
- OpenRouter has no fast endpoint for `claude-haiku-5.5`, so `speed: "fast"` is served at standard speed. Anthropic fast mode exists only on Opus models, which cost much more. Do not add an Opus canary without approval.
- A different tier breaks the provider's prompt cache. The canary prompt is too short to be cached, so this does not change the A/B result.
- `claude-sonnet` routes to Vertex. The flex check stops at routing, so it needs no Google credentials.
