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
| `pi-web-access` | 0.28.0 | Free-first search routing from `config/web-search.json`; optional API keys from workflow environment |

No MCP server has been selected for this action. To add one, change the action-owned MCP configuration in `writeRuntimeConfig` after reviewing that server's access requirements. The action does not import MCP server commands from the reviewed repository or runner home directory.

Oceans mode uses the gateway's `/v1` OpenAI-compatible endpoint and the supplied Oceans key. Direct mode requires a Pi `provider/model` ID and its provider credential in the workflow environment. The `provider-key` input remains part of Oceans configuration resolution; it does not fetch a secret from Oceans. The worker forwards only the explicit credential names listed in `src/pi.ts`, including `EXA_API_KEY`, `BRAVE_API_KEY`, and `TAVILY_API_KEY` for search.

Each review uses a temporary home and Pi configuration directory. GitHub publishing credentials stay in the parent process. The main session has no shell, write, or edit tool. Subagent calls accept only the bounded foreground form described in the prompt. These are tool restrictions, not an operating-system sandbox; use a runner whose filesystem and network access are suitable for the review.

## Search defaults and API keys

The action copies `config/web-search.json` into the isolated Pi directory before loading extensions. The ordered route is:

```json
{
  "firecrawlBaseUrl": "https://api.firecrawl.dev",
  "searchRouting": {
    "providers": ["exa", "parallel-mcp", "tinyfish", "parallel", "openai", "gemini", "perplexity", "firecrawl"],
    "useCurrentModel": true,
    "fallbackOn": ["unsupported", "transient", "quota", "network", "invalid-response"]
  }
}
```

Exa uses anonymous MCP when no key is supplied. `parallel-mcp` selects Parallel's anonymous MCP service; `parallel` selects its authenticated REST API. Providers without the required credentials are skipped. The first successful provider ends the search. Authentication and invalid-request errors stop the route, so a bad key is not silently hidden. The review prompt selects `workflow:"none"` to avoid interactive search curation in CI.

Firecrawl permits anonymous search, but its IP checks can return HTTP 403. A live check from the development runner received that response. Firecrawl is last so this error cannot block the other providers. Exa and Parallel MCP each returned results in the same credential-free check. Shared CI addresses may have different limits; these checks are not an availability guarantee.

`useCurrentModel:true` applies to the OpenAI entry only. It requires an eligible model on an official OpenAI Responses or Codex endpoint. An Oceans OpenAI-compatible route does not meet that condition. Gemini uses its own configured search model and credential. This version of `pi-web-access` has no `anthropic` search provider; an Anthropic model can still use the external search tools. No runner login or browser cookies are imported.

Pass optional keys as workflow environment variables. Keys stay in GitHub Secrets; do not put them in the tracked JSON file. Supplying a key can enable billed usage after any free allowance.

| Provider | Workflow variable | Initial access and setup |
| --- | --- | --- |
| Exa | `EXA_API_KEY` | [Anonymous MCP](https://exa.ai/docs/reference/exa-mcp); a key enables direct API access and higher limits |
| Parallel | `PARALLEL_API_KEY` | [Anonymous MCP](https://parallel.ai/blog/free-web-search-mcp); [REST API](https://docs.parallel.ai/search/search-quickstart) requires a key |
| TinyFish | `TINYFISH_API_KEY` | [Free search](https://docs.tinyfish.ai/search-api), but an account and key are required |
| Firecrawl | `FIRECRAWL_API_KEY` | [Keyless search](https://docs.firecrawl.dev/introduction), subject to IP limits; a key raises limits |
| OpenAI | `OPENAI_API_KEY` | Authenticated first-party search; requires an eligible active OpenAI model for this route |
| Gemini | `GEMINI_API_KEY` or `GOOGLE_API_KEY` | Authenticated Google search grounding; model-specific pricing and allowances apply |
| Perplexity | `PERPLEXITY_API_KEY` | [Authenticated API](https://docs.perplexity.ai/docs/search/quickstart); the no-key playground is not an anonymous API |

For example, add `TINYFISH_API_KEY: ${{ secrets.TINYFISH_API_KEY }}` under the action step's `env`. See the [upstream configuration guide](https://github.com/nicobailon/pi-web-access#configuration) for provider options and local Pi setup. Local Pi normally reads `~/.pi/web-search.json`; this action uses its temporary `PI_CODING_AGENT_DIR/web-search.json` instead.

### Research basis

Checked on 2026-09-06. The [Artificial Analysis study](https://artificialanalysis.ai/articles/search-api) holds the model and harness fixed while comparing search quality, total cost, and time. It shows why per-query price alone is a poor ranking rule. The [TinyFish benchmark page](https://www.tinyfish.ai/benchmarks) reports strong latency and Search + Fetch results, but is vendor-published and measures different tasks. Neither benchmark proves PR review quality or anonymous endpoint availability. The initial order therefore favors verified anonymous access, then optional keyed providers; it is not a claim that one provider wins every benchmark.

## Development and verification

Run from the repository root:

```sh
mise run review-agent-action-check
mise run lint
```

The action check installs from the lockfile, typechecks, runs unit tests, applies the lint and format checks from `actions/.oxlintrc.json` and `actions/.oxfmtrc.json`, and runs a real Pi SDK session against a local deterministic model server. Those configuration files are copies of the admin UI configurations. To format source, run `mise exec -- bun run --cwd actions/review-agent fmt`.

The SDK smoke checks extension tool registration, valid and invalid result handling, and foreground delegation. A separate search smoke exercises the installed package's quota fallback from anonymous Exa to Parallel MCP and verifies that authentication errors remain visible. Both checks are deterministic and do not prove live model quality or external MCP/search connectivity. The manual self-hosted smoke exercises the action lifecycle against a mock Oceans API with GitHub publishing disabled.
