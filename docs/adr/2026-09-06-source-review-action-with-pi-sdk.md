# Source-based review action with the Pi SDK

Status: Accepted

> **Amendment note (2026-10-13):** [Built-in MCP, pi-intercom, and advisory review status](#amendment-built-in-mcp-pi-intercom-and-advisory-review-status) replaces `pi-mcp-adapter` and makes failed reviews advisory. The original decision remains below as history.
>
> **Amendment note (2026-10-13, trigger):** [Review workflow on `pull_request`](#amendment-review-workflow-on-pull_request) replaces the `pull_request_target` trigger described under Consequences.

## Context

The review action used a committed Rolldown bundle and invoked `pi review` with action-specific flags. Pi does not provide that review command. A bundle also adds a build and freshness check to each source change without being required by a composite action.

## Decision

Keep the composite action and run its TypeScript entry point with Node.js 26 and `tsx`. Install exact dependencies from the Bun lockfile at action startup. Remove the bundle and its build and freshness scripts.

Use the Pi SDK in a child process with a temporary home and configuration directory. Keep Oceans configuration, run reporting, and GitHub publishing in the parent. Load the action-owned review rubric and skill, plus the locked `pi-mcp-adapter`, `pi-subagents`, and `pi-web-access` packages. Require a structured `submit_review` call whose finding locations match changed RIGHT-side lines.

Limit the review to read and research tools. Allow bounded foreground subagents with read tools and no child extensions. Do not load project extensions or runner MCP configuration. The initial MCP server registry is empty because no server is required by the review contract.

## Consequences

Consumers can import `actions/review-agent` at a reviewed commit without building it first. Source and runtime dependencies remain reviewable in the same revision. Startup now requires package registry access and takes longer than running a prebuilt bundle. Package updates require a lockfile update and the real SDK smoke check.

The wrapper requires a Linux Bubblewrap boundary with a read-only source mount, isolated runtime cwd, and no procfs. This prevents read tools from inspecting runner files or process environments. Network access remains available for model and search calls. The credentialed workflow runs trusted default-branch code on `pull_request_target`; PR source is inspected only as data. A separate unprivileged workflow tests PR-head action changes with synthetic credentials. This adds a Linux runtime dependency and means new privileged workflow changes become active only after merge. Direct providers use explicit workflow credentials; Oceans credential labels do not retrieve secrets. Live model, search, and MCP service checks remain separate from deterministic CI tests.

Search uses the action-owned `config/web-search.json`. Prefer anonymous Exa and Parallel MCP, then optional keyed providers and first-party search supported by the package. Keep Firecrawl last because its anonymous IP checks can return authentication errors that stop fallback. Provider key setup and benchmark evidence are recorded in the action README. Do not add unsupported provider names such as `anthropic` to the route.

## Verification

`review-agent-action-check` covers type checking, unit tests, lint, formatting, and real SDK calls to a local model mock. The self-hosted workflow covers the run lifecycle with a mock Oceans API and publishing disabled. Hosted workflow execution and live provider quality require deployment checks after the change is published.

## Amendment: built-in MCP, pi-intercom, and advisory review status

Date: 2026-10-13

### Decision

Replace `pi-mcp-adapter` with Pi's built-in MCP extension, loaded through `createMcpExtension()`. Load `pi-intercom` in its place alongside `pi-subagents` and `pi-web-access`.

Treat a failed or unavailable review as advisory. The workflow keeps the job green and publishes a neutral `Oceans review` check that names the failed step.

### Implementation

- Built-in MCP reads only the action-owned `mcp.json` in the temporary agent directory, because the review never trusts its working directory. The server registry stays empty.
- The `pi-intercom` broker lives under the temporary agent directory. Only replies to the review's own messages may start a turn. The worker stops the broker on exit, SIGTERM, and SIGINT.
- Subagents contact the parent through the `pi-subagents` supervisor channel, so their read-only tool ceiling is unchanged.
- Review workflow steps continue on error and are gated on their predecessors. The final step creates or updates a check scoped to the pull request.

### Rationale

Pi 1.x ships MCP support, so the adapter duplicated a built-in capability. Review infrastructure failures, such as a revoked provider key, say nothing about whether a pull request can merge.

### Trade-offs and follow-ups

- The action still reports its own failure as an error annotation inside the green job.
- Checks created with `GITHUB_TOKEN` can appear grouped under another workflow.
- Dependabot-triggered runs are skipped because they receive no secrets and a read-only token.
- If `Oceans review` becomes a required check, `neutral` counts as passing.

## Amendment: review workflow on `pull_request`

Date: 2026-10-13

### Decision

Run `oceans-review-agent.yml` on `pull_request` instead of `pull_request_target`. Keep skipping fork, draft, and Dependabot pull requests. Keep the separate PR-head checkout as review data.

### Rationale

GitHub's workflow execution protections disable `pull_request_target` by default in public repositories and start enforcing that on 2026-11-02. Same-repository pull requests receive secrets and the declared token permissions on `pull_request`. Their authors already have write access and could run a credentialed workflow by pushing a branch. The trusted-branch boundary therefore added little protection. The generated consumer workflow already uses `pull_request`.

### Trade-offs and follow-ups

- Workflow and action changes now take effect in the pull request that makes them, not only after merge.
- A pull request can change the code that receives `OPENROUTER_API_KEY`. Move the key into a protected environment if contributors without write access ever need same-repository branches.
