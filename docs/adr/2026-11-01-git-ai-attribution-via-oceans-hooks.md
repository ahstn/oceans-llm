# ADR Git AI attribution through `oceans hook`

- Date: 2026-11-01
- Status: Proposed

## Context

[Git AI](https://github.com/git-ai-project/git-ai) records which lines of each commit an agent wrote. It stores them as git notes on `refs/notes/ai`, in the open, Apache-2.0 [Git AI Standard v3](https://github.com/git-ai-project/git-ai/blob/main/specs/git_ai_standard_v3.0.0.md). Oceans already records agent sessions, models, tokens and cost from gateway traffic, but not what those sessions committed. Joining the two gives repo and contributor AI-authorship stats and cost per committed AI line.

The two datasets fill each other's gaps. Git AI's Claude Code preset stores the hook's `session_id` as `agent_id.id`, the same value Claude Code sends as `x-claude-code-session-id`. Oceans captures that header ([`agent_analysis.rs`](../../crates/gateway-service/src/agent_analysis.rs)) and stores it as `normalized_session_id`. Claude Code hooks don't report a model, so 1,150 of 1,205 session entries in this repository's notes have `model: "unknown"`, a gap gateway data fills.

Git AI also offers a hosted Public Analytics API (17 operations under `/api/v1`, `x-api-key` auth) and a commercially licensed self-hosted platform. Both need a paid Git AI for Teams account.

## Decision

Integrate through two hook commands in the existing `oceans` CLI, and parse notes on the gateway with our own v3 parser.

```
git-ai (attribution and history rewrites)
   └─ git_ai_hooks.post_notes_updated → oceans hook notes   (note batch → Oceans)
Claude Code hooks                     → oceans hook claude  (session, repo and HEAD context → Oceans)
```

- **`oceans hook notes`** is registered with `git-ai config --add git_ai_hooks.post_notes_updated "oceans hook notes"`. git-ai runs it after every note write with a JSON array of `{commit_sha, repo_url, repo_name, branch, is_default_branch, note_content}` on stdin.
- **`oceans hook claude`** is a Claude Code hook. At session start and end it sends the session id, repo URL, branch, HEAD and harness version. It sends no prompt text.
- Both post to a new authenticated ingestion endpoint under `/api/v1`, using the caller's existing Oceans API key. The endpoint is Oceans' own contract, not a Git AI-compatible one.
- The gateway parses notes against the v3 spec and stores totals per commit and per session. It stores no file paths and no line ranges.

## Implementation

- **Hook budget:** git-ai waits 3 s for hook commands, then detaches them ([`git_ai_hooks.rs`](https://github.com/git-ai-project/git-ai/blob/main/src/authorship/git_ai_hooks.rs)). git-ai's background service writes the notes, so even a slow hook doesn't delay `git commit`. `oceans hook notes` still writes the batch to a local queue and returns, so a service restart or a network failure can't lose it. Delivery happens in the background, with retries.
- **Configuration:** the CLI reads `OCEANS_URL` and `OCEANS_API_KEY` from the environment today. The notes hook runs under git-ai's background service and the Claude hook under Claude Code, neither of which reliably has the user's shell environment. Putting the key in git-ai's hook command would store it in `~/.git-ai/config.json`, which other tools read. The CLI therefore gains a config file, described in [CLI configuration and key resolution](#cli-configuration-and-key-resolution). git-ai reads its own config once per process, so installing the hook also means running `git-ai bg restart`.
- **Repo identity:** git-ai passes the default remote's URL unchanged, which can include credentials (`https://token@host/...`). The CLI strips userinfo and normalises the URL to `host/org/repo` before it leaves the machine, and the gateway rejects URLs that still have userinfo.
- **Parser:** the parser is a dependency-light module, roughly 200–400 lines, that accepts all three attestation key formats (legacy hex, `s_…::t_…`, `h_…`). It counts attested lines per session and per known human, and keeps `agent_id` and `human_author`. Unattested added lines stay unknown, not human.
- **Join:** a session joins an Oceans agent session on `(harness, normalized_session_id)` within the caller's ownership scope. Oceans stores the `x-claude-code-session-id` value verbatim, so a main-session id in a note matches it without any transformation. Lines written by a Claude Code subagent carry the subagent's transcript name (`agent-<hex>`) instead. These join on the request's `execution_id`, which Oceans takes from `x-claude-code-agent-id`, once the header's format is confirmed. Until then they count as AI lines without a matching session. A match is correlation evidence, not identity, as in the [passive agent session analysis ADR](2026-07-21-passive-agent-session-analysis.md). Gateway data supplies the model and cost when the note says `unknown`.
- **Idempotency:** rewrites (rebase, amend, cherry-pick) produce new notes for new SHAs. Ingestion is keyed on `(repository, commit_sha)` and the latest note for a commit replaces the earlier one.
- **Storage:** paired LibSQL and PostgreSQL schemas, following existing store conventions.

## CLI configuration and key resolution

`oceans` reads `~/.config/oceans/config.yaml`, or `$XDG_CONFIG_HOME/oceans/config.yaml` when that variable is set. YAML matches the gateway's own `gateway.yaml`.

```yaml
url: https://oceans.example.com
api_key: "!fnox get OCEANS_INGEST_KEY"   # or "env:OCEANS_INGEST_KEY", or a literal key
```

- **Precedence:** a command-line flag wins, then the `OCEANS_URL` and `OCEANS_API_KEY` environment variables, then the file. Shell and CI use behave as they do today.
- **`api_key` forms:** the value is resolved in this order. This follows [OMP's `apiKey` resolution](https://omp.sh/docs/custom-models), except that a variable reference must be explicit.
  1. A value starting with `!` is a shell command, and its trimmed stdout is the key.
  2. A value starting with `env:` names an environment variable. If that variable is unset or empty, resolution fails with an error naming it.
  3. Anything else is the literal key.
- **No fallback from a variable name to a literal key:** OMP uses a bare variable name as the literal key when the variable is unset. Hooks usually run without the user's environment, so that rule would send the variable's name as the key and surface as an unexplained 401.
- **YAML quoting:** an unquoted scalar starting with `!` is a YAML tag, so the command form must be quoted. A tagged value is rejected with an error that shows the quoted form.
- **Resolve in the sender, not the hook:** `oceans hook notes` and `oceans hook claude` only queue their payloads and never resolve the key. The background sender resolves it with a 10 s timeout and keeps the result in memory for that run only. If resolution fails, the queue is kept and retried. A command's output is never logged; errors report only its exit status.
- **File permissions:** `oceans` refuses to run a `!` command, or to read a literal key, from a config file that other users can write to, because the file effectively becomes code that runs on every commit.
- **`oceans login`:** this command writes the file with owner-only permissions. It can create an ingest-only [scoped API key](2026-10-17-scoped-api-keys.md) for the hooks, so a leaked file or command output can only submit attribution data.

## Hook verification

Tested on 2026-11-01 with git-ai 1.7.5, using a separate git-ai background service with its own HOME, a dummy hook that logged its stdin, and `git-ai checkpoint mock_ai` standing in for agent edits.

| Operation | Hook ran | Payload |
|---|---|---|
| Commit with AI lines | Yes | The new commit |
| `commit --amend` | Yes | The amended commit |
| `rebase` (2 commits) | Yes, once | Both rewritten commits in one batch |
| `cherry-pick` | Yes | The new commit |
| `reset --soft` then commit | Yes | The new commit |
| `stash`, `stash pop`, commit | Yes | The new commit, with the stashed AI lines |
| `merge --squash` then commit | Yes | The squash commit, with lines from both branch commits |
| Human-only or unchecked commit | Yes | A note with only `h_` attestations |
| `merge --no-ff` | No | No note is written |
| `revert` | No | No note is written |
| Notes from a teammate's push, then `fetch` and `pull` | No | The note arrives locally, but the hook doesn't run |
| Hook that takes 8 s | Yes | `git commit` returned at once, and the hook finished after git-ai detached it |

Every payload carried the remote URL exactly as configured, including the `user:token@` part.

## Session id verification

Checked on 2026-11-01 with Claude Code 2.1.309. No Oceans gateway records Claude Code traffic on the maintainer's machine, so each link was checked separately.

- **Notes and Claude Code:** this repository's notes contain 89 distinct Claude session ids. 66 are the names of local Claude Code transcripts. 20 are subagent ids (`agent-<hex>`), the name of a transcript under the parent session's `subagents/` folder. git-ai uses that name when the hook payload has no `session_id`. The remaining 3 have no local transcript.
- **Claude Code and the gateway header:** a `claude -p` run sent through a loopback proxy carried `x-claude-code-session-id: e72a8b10-c0ba-4895-8a5b-16cccdd0cca7`. That was also the hook's `session_id` and the transcript's name.
- **Gateway normalisation:** `client_session.rs` keeps the header value unchanged and only rejects empty, oversized or non-identifier values, so a UUID is stored as sent.
- **Not verified:** the values a subagent's requests send in `x-claude-code-agent-id` and `x-claude-code-parent-agent-id`. The run that would have spawned a subagent was refused by the API before any tokens were used. Subagent transcripts record `agentId` as bare hex, without the `agent-` prefix git-ai adds.

## Why

- **Git AI already handles the hard part:** git-ai is about 175k lines of Rust, most of it moving attribution through rebase, squash, cherry-pick, stash and amend. Its documented `post_notes_updated` hook gives us the result of that work without depending on its internals.
- **No paid dependency:** the hook and the notes format work with free, local Git AI. The Analytics API and self-hosted platform both need a Git AI for Teams licence.
- **Joins with data Oceans already holds:** the hook runs on the machine that also sends gateway traffic, under the same Oceans API key. Ownership scope and the session-id join line up without mapping identities across systems.
- **Stable format:** the v3 spec promises not to change the note structure or key prefixes without a major version bump.

## Trade-offs

- **Only machines with the hook report:** commits from machines without `oceans hook`, and history from before it was installed, are missing until the server-side clone backfill described below exists.
- **Remote-merged notes don't fire the hook:** git-ai merges notes fetched from a remote via `merge_notes_from_ref`, which skips the hook. Each machine reports its own notes, so team coverage depends on everyone installing the hook.
- **Merge and revert commits have no notes:** git-ai writes no note for `git merge --no-ff` or `git revert`, so they report nothing. The commits a merge brings in already have notes of their own.
- **Hosted notes mode is unsupported:** in git-ai's `hosted` notes backend, notes go to Git AI's servers and the hook doesn't run.
- **Squash and rebase merges:** hosted SCMs don't run git-ai, so attribution on squash- or rebase-merged commits survives only where Git AI's CI action runs. If that action should report too, it needs `oceans hook notes` and a service-account key in CI.
- **Local queue:** a background queue adds state on developer machines (spool, retries, cleanup) that a synchronous post would not.
- **Keys on disk:** a literal `api_key` sits in plain text, like `gh` or AWS credentials. The `!` and `env:` forms avoid this, and an ingest-only scoped key limits what a leaked key can do. OS keychain storage is not included; background hooks can hit locked keyrings or authorization prompts.
- **`serde_yaml` in the CLI:** reading YAML adds `serde_yaml` to the distributed `oceans` binary. The gateway already depends on it, but upstream no longer maintains it.
- **git-ai behaviour can change:** the hook name, payload and 3 s wait are documented but not covered by the v3 standard's stability guarantees.

## Rejected Alternatives

### Consume Git AI's Analytics API

Rejected as the default because it needs a paid Git AI for Teams account, and Oceans would only re-display numbers Git AI's dashboard already shows. It may return later as an optional adapter.

### Emulate the git-ai CLI's `/worker/*` endpoints

Rejected. They are undocumented, change with CLI releases, and are only used in modes that send data to Git AI's servers. In the default `git_notes` mode the CLI sends nothing to any server without a key.

### Use the git-ai crate as a library

Rejected. It isn't published on crates.io (that name belongs to an unrelated project), `lib.rs` exposes CLI internals with no stability promise, rewrite tracking depends on its daemon, and it adds about 60 dependencies, including a bundled `rusqlite` that may clash with libsql.

### Reimplement git-ai

Rejected. The notes format is open, and attribution through history rewrites is most of git-ai's code. Owning it gains nothing.

### Run `git ai stats` on the server

Rejected for ingestion. On this repository, `git ai stats HEAD~200..HEAD` hadn't finished after nearly 4 minutes, while `git cat-file --batch` read all 1,037 notes in 0.03 s.

## Follow-ups

- Capture `x-claude-code-agent-id` and `x-claude-code-parent-agent-id` from a Claude Code run that spawns a subagent, and confirm how they relate to git-ai's `agent-<hex>` ids. In this repository, 20 of 89 Claude session ids in notes are subagent ids.
- Check whether Codex session ids in notes match the gateway's Codex `session-id` values.
- Confirm the join on a deployment where a gateway records Claude Code traffic for the same commits.
- Server-side clone backfill as an optional source for history and for machines without the hook. Admin-entered clone URLs need allowlisted SCM hosts and the SSRF rules in [SSRF-safe outbound fetch](2026-10-17-ssrf-safe-outbound-fetch.md).
- Optionally serve a read-only subset of the Analytics API shape so external tools, such as Git AI's DevLake plugin, can read Oceans. Check Git AI's terms before copying their spec.
- Add admin routes, `features/*.md` recipes and `features/routing.json` entries when the UI lands.
