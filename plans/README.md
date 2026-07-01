# Review Agent Implementation Plans

Generated on 2026-06-30 from GitHub issues:

- #192: Review Agent subsystem epic
- #193: persistence model
- #194: admin and action APIs
- #195: reusable TypeScript GitHub Action

Planning baseline: commit `77666d5`.

Note: the requested root `PLAN.md` was not present in this checkout. `README.md`
and `PLAN-boring-avatars.md` were reviewed for project context.

## Execution Order

1. [001-review-agent-persistence.md](001-review-agent-persistence.md)
2. [002-review-agent-admin-action-apis.md](002-review-agent-admin-action-apis.md)
3. [003-review-agent-action-package.md](003-review-agent-action-package.md)

The API plan depends on the persistence plan. The action package can be
scaffolded against mocked APIs once the #194 DTOs are settled, but full dogfood
testing needs all three plans.

## Cross-Cutting Decisions

- Use provider-neutral `review_agent_*` naming with `provider = github`.
- Do not create dedicated Review Agent token tables or endpoints for MVP.
  Issue #194 supersedes the early #193 token idea: action auth uses existing
  service account API keys.
- Bind each configured repository to a `service_account_id`.
- Persist metadata, config snapshots, sanitized metrics, and short error
  summaries only. Do not persist raw diffs, code blobs, prompts, transcripts,
  model outputs, or full generated review text.
- Keep the action-executes, Oceans-controls model: GitHub Actions runs Pi and
  publishes results, while Oceans resolves config, validates safety invariants,
  tracks runs, and stores sanitized metrics.
- Generated consumer workflows must use `pull_request`, same-repo head guards,
  skip drafts, least-privilege permissions, checkout PR head, concurrency
  cancellation, and GitHub secrets placeholders.

## Testing Options Shortlist

### 1. Self-Hosted Runner on `godrics-hollow` (Recommended Dogfood Path)

Run a repo-scoped GitHub self-hosted runner on `godrics-hollow` and run Oceans
on the same host or LAN. The workflow can call `http://localhost:<port>` or a
private LAN URL, so no public tunnel is needed for the first dogfood loop.

Why this is feasible:

- It exercises real GitHub Actions event payloads, `GITHUB_TOKEN`, checkout,
  permissions, job summaries, and PR comment publishing.
- It avoids exposing a local Oceans instance to the public internet.
- GitHub's self-hosted runner model supports local services and custom host
  environments.

Risks and mitigations:

- A self-hosted runner has access to local resources. Use a repo-scoped runner,
  a dedicated OS user, minimal labels, and only same-repo pull requests.
- It does not prove that GitHub-hosted runners can reach Oceans. Use option 2
  for that external-network smoke test.

Use this for the first end-to-end acceptance run for #192.

### 2. GitHub-Hosted Runner Plus Cloudflare Named Tunnel

Expose the Oceans instance on `godrics-hollow` through a Cloudflare named tunnel
and point the workflow's `oceans-url` at the public HTTPS hostname.

Why this is feasible:

- It validates the consumer experience from a standard GitHub-hosted runner.
- It proves the action works when Oceans is reachable only through a public URL.
- Cloudflare Tunnel supports mapping a public hostname to a local HTTP service.

Risks and mitigations:

- This exposes a homelab service path. Use a temporary named tunnel, a dedicated
  Review Agent service account API key, tight route scope, logging, and teardown
  after testing.
- Quick tunnels are for testing and random hostnames; use a named tunnel for a
  repeatable dogfood workflow.

Use this after option 1 to validate the public-runner path.

### 3. Local Runner Emulation for Inner Loop

Use local tooling for fast action iteration before real GitHub runs:

- `redwoodjs/agent-ci`: more faithful runner emulation because it uses the
  official GitHub runner binary and local API emulation.
- `nektos/act`: mature Docker-based workflow runner for local smoke tests.
- `@github/local-action` or direct Node execution: focused testing for the
  JavaScript action entrypoint.

Why this is feasible:

- It is fast enough for packaging, input parsing, preflight, lifecycle API mocks,
  redaction, and dist/no-install checks.
- It avoids burning GitHub Actions minutes for every action edit.

Risks and mitigations:

- `agent-ci` currently documents that local `steps[*].uses: ./...` actions are
  unsupported, so it may need a branch/SHA action reference or direct local
  action execution.
- `act` is useful but not a final proof for GitHub API permissions, hosted
  networking, or runner parity.

Use this for #195 development checks, then use options 1 and 2 for acceptance.

## Sources Researched

- GitHub issue context fetched with `gh issue view --comments`: #192, #193,
  #194, #195.
- GitHub Docs: self-hosted runners and JavaScript action packaging.
- Cloudflare Docs: Tunnel setup and public hostname routing.
- `redwoodjs/agent-ci` repository and compatibility notes.
- `nektos/act` repository and local runner behavior.
