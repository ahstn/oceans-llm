# Source-based review action with the Pi SDK

Status: Accepted

## Context

The review action used a committed Rolldown bundle and invoked `pi review` with action-specific flags. Pi does not provide that review command. A bundle also adds a build and freshness check to each source change without being required by a composite action.

## Decision

Keep the composite action and run its TypeScript entry point with Node.js 24 and `tsx`. Install exact dependencies from the Bun lockfile at action startup. Remove the bundle and its build and freshness scripts.

Use the Pi SDK in a child process with a temporary home and configuration directory. Keep Oceans configuration, run reporting, and GitHub publishing in the parent. Load the action-owned review rubric and skill, Pi's built-in MCP extension, and the locked `pi-intercom`, `pi-subagents`, and `pi-web-access` packages. Require a structured `submit_review` call whose finding locations match changed RIGHT-side lines.

Limit the review to read and research tools. Allow bounded foreground subagents with read tools and no child extensions. Do not load project extensions or runner MCP configuration. Built-in MCP reads only the action-owned `mcp.json` in the temporary agent directory, because the review never trusts its working directory. The initial MCP server registry is empty because no server is required by the review contract. `pi-intercom` replaced `pi-mcp-adapter` once Pi shipped built-in MCP. Its broker lives under the temporary agent directory, only replies to the review's own messages may start a turn, and the worker stops the broker on exit. Subagents contact the parent through the `pi-subagents` supervisor channel, so their read-only tool ceiling is unchanged.

## Consequences

Consumers can import `actions/review-agent` at a reviewed commit without building it first. Source and runtime dependencies remain reviewable in the same revision. Startup now requires package registry access and takes longer than running a prebuilt bundle. Package updates require a lockfile update and the real SDK smoke check.

The wrapper requires a Linux Bubblewrap boundary with a read-only source mount, isolated runtime cwd, and no procfs. This prevents read tools from inspecting runner files or process environments. Network access remains available for model and search calls. The credentialed workflow runs trusted default-branch code on `pull_request_target`; PR source is inspected only as data. A separate unprivileged workflow tests PR-head action changes with synthetic credentials. This adds a Linux runtime dependency and means new privileged workflow changes become active only after merge. Direct providers use explicit workflow credentials; Oceans credential labels do not retrieve secrets. Live model, search, and MCP service checks remain separate from deterministic CI tests. A failed or unavailable review is advisory: the workflow keeps the job green and publishes a neutral `Oceans review` check that names the failed step, because review infrastructure failures say nothing about whether the PR can merge.

Search uses the action-owned `config/web-search.json`. Prefer anonymous Exa and Parallel MCP, then optional keyed providers and first-party search supported by the package. Keep Firecrawl last because its anonymous IP checks can return authentication errors that stop fallback. Provider key setup and benchmark evidence are recorded in the action README. Do not add unsupported provider names such as `anthropic` to the route.

## Verification

`review-agent-action-check` covers type checking, unit tests, lint, formatting, and real SDK calls to a local model mock. The self-hosted workflow covers the run lifecycle with a mock Oceans API and publishing disabled. Hosted workflow execution and live provider quality require deployment checks after the change is published.
