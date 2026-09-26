# Oceans Admin verification map

This directory is the maintained source for verification of the user-facing Oceans LLM admin control plane. Read this index before driving the app, then use the matching feature file as the recipe.

## Baseline preconditions

- Start the real gateway and admin UI with `control-oceans-admin launch`, which runs `mise run dev-stack` with the recorded alternate ports exported as `GATEWAY_PORT` and `UI_PORT`.
- Use only the stack started for the current `OCEANS_VERIFY_RUN_ID`.
- Require `control-oceans-admin doctor` to report the recorded gateway PID, ready state, and gateway version.
- Use the seeded `admin@local` / `admin` platform-admin account unless a feature requires a narrower role.
- Verify Agent Sessions with the seeded platform administrator. The page needs `agent_sessions` in `gateway.yaml` plus agent-analysis access, which `dev-stack` grants through `AGENT_ANALYSIS_SHADOW_DIAGNOSTICS_ENABLED=true`.
- Expect `dev-stack` to refresh the demo data in this checkout's `gateway.db`.
- Do not run a second stack from this checkout. It would share `gateway.db` even if its ports differ.

## Routing

[routing.json](./routing.json) maps repository paths to the features below. `control-oceans-admin plan` reads it.

- `features[]` pairs a feature `id` with its recipe `file`, its `proof` (`{"drive": "<target>"}` or `{"manual": true}`), an optional `paid` flag, and path globs.
- `shared[]` sends cross-cutting paths such as the admin shell, `gateway.yaml`, and the core inference path to a fixed smoke set of features.
- `skip[]` lists paths with no local runtime surface, each with a reason. Skip rules win over feature matches.
- Globs support `*`, `**`, `?`, and `{a,b}`.

When you add a recipe or a UI route, add its routing entry in the same change. `scripts/plan-verification.test.mjs` fails when a recipe or an admin route file is unrouted.

## Driving conventions

- Start every recipe from the baseline state unless its preconditions say otherwise.
- Prefer ARIA roles and accessible names over CSS selectors. Use an existing `data-testid` when the visible table has no unique accessible name.
- Treat each command and quoted label as literal.
- Use `control-oceans-admin drive models` for the automated Models proof.
- Use `control-oceans-admin drive observability` for the combined Leaderboard and Agent Harnesses proof.
- Use `control-oceans-admin drive backend-gateway` for the bounded OpenRouter, deterministic guardrail, generated-tool decision, and request-log proof.
- Use `control-oceans-admin drive mcp` for the MCP registry, Tool Sets workbench, grant checks, bounded live tool calls, and invocation proof. Read its credential preconditions before launch.
- Extend the harness before reporting another path as automated. Manual Playwright steps in this map remain the contract for that extension.
- Read-only control-plane verification does not call upstream services. Use the live LLM recipe when changed model-request behavior warrants paid integration proof. Use the MCP recipe for authorized, reviewed read-only tool calls; these can consume upstream service quota. Neither recipe permits unrelated upstream mutations.

## Proof and skip reporting

- Capture the user entry action and resulting state, not only the final screen.
- UI proof includes an ARIA snapshot and a screenshot with `Oceans Gateway` or the page heading visible.
- Read-only data proof compares a visible list or detail with the production API used by the UI.
- Mutation proof must include a second read-only view of the saved value and cleanup of the created record.
- Record the feature ID, entry point, run ID, gateway version, and artifact directory.
- Report an unreachable path with the attempted action and unmet precondition.
- Do not report a skipped entry point as verified through a different path.
- Keep proof artifacts after stack cleanup.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph that describes user-visible behavior. It then uses exactly four H2 sections in this order.

1. `Sub-features` lists short IDs and one line for each behavior.
2. `How to get to it (user POV)` lists each user entry point.
3. `Driving it with control-oceans-admin` starts with `Preconditions:` and pairs each action with a stable handle and observable result.
4. `Gotchas` lists traps that can invalidate a verification run.

## Features

- [Models](./models.md) covers password sign-in, sidebar navigation, configured model listing, and model detail.
- [Leaderboard](./leaderboard.md) covers 7-day production API parity, top models, the most-used harness, and the 31-day range.
- [Agent Harnesses](./agent-harnesses.md) covers request and token aggregates, Mastra and Oh My Pi presentation, and the 31-day range.
- [Password sign-in](./password-sign-in.md) covers protected-route redirection, seeded credentials, authenticated identity, and sign-out.
- [API keys](./api-keys.md) covers the scoped key list, create and manage flows, one-time user-key secrets, and authorized service-account reveal controls.
- [Agent sessions](./agent-sessions.md) covers the seeded session list, filters, and detail sheet.
- [Request logs](./request-logs.md) covers the seeded request list, filters, and request detail.
- [Live LLM requests](./live-llm-requests.md) covers bounded paid canaries through OpenRouter or Bedrock and their request-log evidence.
- [Backend gateway](./backend-gateway.md) covers the OpenRouter route for `deepseek-v4-flash-0731`, deterministic and generated-tool guardrails, request-log evidence, and temporary key cleanup.
- [Usage costs](./usage-costs.md) covers the spend KPIs, the 7- and 30-day windows, the owner filter, breakdowns, FOCUS export, and spend-report API parity.
- [Spend controls](./spend-controls.md) covers user, service-account, and model budgets, alert history, and a safe model-budget mutation with cleanup.
- [Guardrails](./guardrails.md) covers effective policy cards, decision filters, and a free evaluate-endpoint decision.
- [Batch requests](./batch-requests.md) covers the seeded batch list, filters, the responses sheet, and the cancel dialog without cancelling.
- [Identity](./identity.md) covers the Users, Teams, and Service Accounts lists, user management, team membership transfer, and read-only directories.
- [Account onboarding](./account-onboarding.md) covers password-user creation, the invite URL, `Account ready`, and voluntary password change.
- [Review agent](./review-agent.md) covers repository registration, review settings, workflow generation, run listing, and disable cleanup.
- [Workspace connections](./workspace-connections.md) covers the personal OAuth MCP connections page and its default empty state.
- [MCP](./mcp.md) covers registry discovery, Tool Sets, generated client configuration, explicit grants, direct and aggregate tool calls, upstream authentication failure, invocation records, and cleanup.
