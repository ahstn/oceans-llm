# Plan 003: Reusable Review Agent GitHub Action

Issue: #195
Depends on: [002-review-agent-admin-action-apis.md](002-review-agent-admin-action-apis.md) for live API integration
Suggested branch: `codex/review-agent-action`

## Goal

Package a reusable JavaScript/TypeScript action at `actions/review-agent` that
consumers can call as:

```yaml
uses: ahstn/oceans-llm/actions/review-agent@<ref>
```

The action should require no dependency install in consumer workflows. It should
ship a checked-in `dist/index.js`, validate PR safety conditions locally, call
Oceans for config/lifecycle reporting, invoke Pi, and publish or report sanitized
results according to effective config.

## Package Layout

Create:

- `actions/review-agent/action.yml`
- `actions/review-agent/package.json`
- `actions/review-agent/bun.lock`
- `actions/review-agent/src/main.ts`
- `actions/review-agent/src/input.ts`
- `actions/review-agent/src/github-context.ts`
- `actions/review-agent/src/preflight.ts`
- `actions/review-agent/src/oceans-client.ts`
- `actions/review-agent/src/run-lifecycle.ts`
- `actions/review-agent/src/pi.ts`
- `actions/review-agent/src/result-artifact.ts`
- `actions/review-agent/src/redaction.ts`
- `actions/review-agent/src/summary.ts`
- `actions/review-agent/src/*.test.ts`
- `actions/review-agent/dist/index.js`

Add `mise.toml` tasks for action install/build/test/check so repo tooling stays
consistent:

- `review-agent-action-install`
- `review-agent-action-build`
- `review-agent-action-test`
- `review-agent-action-check`

Wire `review-agent-action-check` into the appropriate lint/test path once the
package exists.

## Runtime and Bundling

Use Bun for package scripts and tests.

For `action.yml` runtime:

- Prefer `node24` only if GitHub Action metadata officially supports it at
  implementation time.
- Otherwise use `node20`, which is the currently documented JavaScript action
  runtime in GitHub's action authoring docs.

Bundler preference:

1. Rolldown
2. Bun build
3. `@vercel/ncc`

The chosen bundler must produce one checked-in `dist/index.js` that runs without
installing dependencies in the consumer repository. Do not commit raw
`node_modules`.

Add a dist drift test that fails when source changes are not reflected in
`dist/index.js`.

## Inputs

Required:

- `oceans-url`
- `oceans-api-key`

Optional:

- `model-id`
- `model-mode`
- direct provider credentials inputs, only if #194 config resolution allows them
- `inline-review`
- `pr-summary`
- `diagrams`
- `linked-issue-detection`
- `linked-issue-assessment`
- `timeout-minutes`
- `max-inline-comments`
- `request-changes-on-high-severity`
- `dry-run`
- `debug`
- `pi-binary`
- `github-token`

Default `github-token` from `github.token` / `GITHUB_TOKEN`.

Mark `oceans-api-key`, `github-token`, and provider credentials with
`core.setSecret` before any logging.

If no effective model is available from input or config resolution, warn and
skip neutrally where possible.

## Local Preflight

Before calling Pi:

- require `pull_request` event
- reject or neutrally skip fork PRs
- skip draft PRs
- read owner/repo/PR number/head SHA/base SHA from the event payload
- verify checkout exists
- verify checked-out `HEAD` matches the PR head SHA
- verify GitHub token can access the target PR before doing expensive work
- validate `oceans-url` and required credentials

Skips should use warnings plus job summary content and a non-failing exit. If a
run has already been started in Oceans, report `skipped`.

## Oceans API Flow

1. Parse and redact inputs.
2. Run local preflight.
3. `POST /api/v1/review-agent/action/config/resolve`.
4. If config says to skip, write a neutral summary and stop.
5. `POST /api/v1/review-agent/action/runs`.
6. Invoke Pi with temporary config/context/result paths.
7. Read the fixed result artifact path.
8. Publish inline comments and/or managed summary according to effective config.
9. `POST /complete` with sanitized metrics.
10. On failure after run start, always attempt `POST /fail`.

The action should never place secrets, prompts, raw diffs, raw code, full model
outputs, or full review text in logs, job summaries, or Oceans reporting
payloads.

## Pi Invocation

Use temp files rather than giant shell prompts:

- context JSON
- effective config JSON
- output artifact path

Pass a minimal environment. Redact all command logging. Support `pi-binary` as
an explicit override for local and CI tests.

Stop condition: if Pi and `pi-subagents` cannot be packaged as action
dependencies/artifacts without runtime installation, document the packaging
blocker and choose one of these follow-ups:

- publish or vendor a minimal Pi runtime artifact consumed by this action
- require `pi-binary` for the first dogfood run
- split the action into a wrapper now and a packaged Pi runtime follow-up

Do not silently add runtime dependency installation to consumer workflows.

## GitHub Publishing

Publishing should support:

- inline findings, bounded by `max-inline-comments`
- one managed top-level PR comment with a hidden marker and update-in-place
  behavior
- optional request-changes behavior for high severity, disabled by default
- dry-run mode with no publishing

Use `@actions/github`/Octokit for PR comments and reviews. Treat comment
publishing failures as reporting failures only when the feature is required by
effective config; optional features should degrade and report degraded status.

## Tests

Bun unit tests:

- input parsing and defaults
- secret redaction
- event/preflight validation
- same-repo and draft skip behavior
- checkout SHA validation
- config resolve request/response handling
- lifecycle start/complete/fail sequencing
- fail reporting after Pi errors
- Pi command construction and temp file behavior
- result artifact parsing and sanitization
- managed comment marker/update logic with mocked Octokit
- dist drift

Mocked integration test:

- fake Oceans API server
- fake `pi-binary` writes a result artifact
- action runs resolve -> start -> fake Pi -> publish mock -> complete
- failure path reports fail

Clean checkout/no-install smoke:

- copy only tracked action files into a temp checkout or use `git archive`
- do not run `bun install`
- execute `node actions/review-agent/dist/index.js` with mocked GitHub/Oceans
  environment and fake `pi-binary`
- prove runtime dependencies resolve from the bundled dist

Local runner tests:

- Use `@github/local-action` or direct Node execution for focused JS action
  behavior.
- Use `agent-ci` or `act` only as inner-loop workflow checks. Do not treat them
  as final acceptance for GitHub permissions or network behavior.

## Verification

```sh
eval "$(/Users/ahstn/.local/bin/mise activate zsh)"
mise run review-agent-action-install
mise run review-agent-action-build
mise run review-agent-action-test
mise run review-agent-action-check
mise run lint
```

Dogfood acceptance should then use:

1. self-hosted runner on `godrics-hollow`
2. GitHub-hosted runner through a Cloudflare named tunnel
