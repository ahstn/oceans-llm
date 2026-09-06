# Review agent action

This composite GitHub Action runs TypeScript source with Node.js 24 and `tsx`. It installs the exact dependencies in `bun.lock` with Bun 1.3.14. There is no build step or committed JavaScript bundle.

The action resolves the repository configuration from Oceans, records a run, and starts a Pi SDK session. Pi reads the commit diff and uses the review rubric in `prompts/review.md` and `skills/code-review/SKILL.md`. The `submit_review` tool validates each finding against changed RIGHT-side lines. The action then publishes the review and reports run metrics to Oceans. A missing or invalid submission fails the run.

## Use from another repository

Replace `ACTION_COMMIT_SHA` with a reviewed commit of this repository that contains the source action. The target checkout must match the PR head and include the base history. Drafts and forks are skipped.

```yaml
name: Oceans review
on: pull_request
permissions:
  contents: read
  pull-requests: write
jobs:
  review:
    if: github.event.pull_request.head.repo.full_name == github.repository
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
        with:
          ref: ${{ github.event.pull_request.head.sha }}
          fetch-depth: 0
          persist-credentials: false
      - uses: ahstn/oceans-llm/actions/review-agent@ACTION_COMMIT_SHA
        with:
          oceans-url: ${{ vars.OCEANS_URL }}
          oceans-api-key: ${{ secrets.OCEANS_REVIEW_API_KEY }}
          github-token: ${{ github.token }}
          dry-run: 'true'
        env:
          EXA_API_KEY: ${{ secrets.EXA_API_KEY }}
```

Set `dry-run` to `false` to publish. Dry runs still call the model and record the run in Oceans. See `action.yml` for all inputs. The action source runs from `github.action_path`; the reviewed repository is `GITHUB_WORKSPACE`.

## Pi packages and credentials

The SDK and the following packages are local, locked dependencies. The resource loader loads these three extensions explicitly and reports any load failure.

| Package | Version | Review configuration |
| --- | --- | --- |
| `@earendil-works/pi-coding-agent` | 0.85.1 | In-memory session with an action-owned prompt, skill, and result tool |
| `pi-mcp-adapter` | 2.32.1 | Exclusive per-run MCP configuration; empty server registry by default |
| `pi-subagents` | 0.66.0 | Foreground reviewer, scout, or oracle; read tools only; depth 1; four spawns per run |
| `pi-web-access` | 0.28.0 | Search, fetch, and search-content tools; optional search API keys from workflow environment |

No MCP server has been selected for this action. To add one, change the action-owned MCP configuration in `writeRuntimeConfig` after reviewing that server's access requirements. The action does not import MCP server commands from the reviewed repository or runner home directory.

Oceans mode uses the gateway's `/v1` OpenAI-compatible endpoint and the supplied Oceans key. Direct mode requires a Pi `provider/model` ID and its provider credential in the workflow environment. The `provider-key` input remains part of Oceans configuration resolution; it does not fetch a secret from Oceans. The worker forwards only the explicit credential names listed in `src/pi.ts`, including `EXA_API_KEY`, `BRAVE_API_KEY`, and `TAVILY_API_KEY` for search.

Each review uses a temporary home and Pi configuration directory. GitHub publishing credentials stay in the parent process. The main session has no shell, write, or edit tool. Subagent calls accept only the bounded foreground form described in the prompt. These are tool restrictions, not an operating-system sandbox; use a runner whose filesystem and network access are suitable for the review.

## Development and verification

Run from the repository root:

```sh
mise run review-agent-action-check
mise run lint
```

The action check installs from the lockfile, typechecks, runs unit tests, applies the lint and format checks from `actions/.oxlintrc.json` and `actions/.oxfmtrc.json`, and runs a real Pi SDK session against a local deterministic model server. Those configuration files are copies of the admin UI configurations. To format source, run `mise exec -- bun run --cwd actions/review-agent fmt`.

The SDK smoke checks extension tool registration, valid and invalid result handling, and foreground delegation. It does not prove live model quality or external MCP/search connectivity. The manual self-hosted smoke exercises the action lifecycle against a mock Oceans API with GitHub publishing disabled.
