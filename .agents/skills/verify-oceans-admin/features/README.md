# Oceans Admin verification map

This directory is the maintained source for verification of the user-facing Oceans LLM admin control plane. Read this index before driving the app, then use the matching feature file as the recipe.

## Baseline preconditions

- Start the real gateway and admin UI with `control-oceans-admin launch`, which runs `mise run dev-stack` from a temporary port-adjusted copy of `gateway.yaml` on recorded alternate ports.
- Use only the stack started for the current `OCEANS_VERIFY_RUN_ID`.
- Require `control-oceans-admin doctor` to report the recorded gateway PID, ready state, and gateway version.
- Use the seeded `admin@local` / `admin` platform-admin account unless a feature requires a narrower role.
- Verify Agent Sessions with the seeded platform administrator. The current `gateway.yaml` grants `agent_sessions` directly to platform administrators.
- Expect `dev-stack` to refresh the demo data in this checkout's `gateway.db`.
- Do not run a second stack from this checkout. It would share `gateway.db` even if its ports differ.

## Driving conventions

- Start every recipe from the baseline state unless its preconditions say otherwise.
- Prefer ARIA roles and accessible names over CSS selectors. Use an existing `data-testid` when the visible table has no unique accessible name.
- Treat each command and quoted label as literal.
- Use `control-oceans-admin drive models` for the automated Models proof.
- Use `control-oceans-admin drive observability` for the combined Leaderboard and Agent Harnesses proof.
- Use `control-oceans-admin drive backend-gateway` for the bounded OpenRouter, deterministic guardrail, generated-tool decision, and request-log proof.
- Extend the harness before reporting another path as automated. Manual Playwright steps in this map remain the contract for that extension.
- Do not mutate provider credentials or call a live provider during read-only control-plane verification. Use the live LLM request recipe only when the changed request behavior warrants paid integration proof.

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
