# ADR Git AI attribution through `oceans hook`

- Date: 2026-11-01
- Status: Accepted
- Amended in part: 2026-11-15, by [Amendment: key owner as note author](#amendment-2026-11-15-key-owner-as-note-author) and [Amendment: request persistence and journal identity](#amendment-2026-11-15-request-persistence-and-journal-identity)
- Extended: 2026-11-15, by [Amendment: report insights for the Attribution tabs](#amendment-2026-11-15-report-insights-for-the-attribution-tabs)

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
OMP extension (oceans-git-ai.ts)      → git-ai checkpoint pi (marks OMP edits for git-ai)
                                      → oceans hook omp     (session, repo and HEAD context → Oceans)
oceans hook prs (local gh)                                  (merged PRs and their commit SHAs → Oceans)
```

- **`oceans hook notes`** is registered with `git-ai config --add git_ai_hooks.post_notes_updated "oceans hook notes"`. git-ai runs it after every note it writes locally, including after rewrites, with a JSON array of `{commit_sha, repo_url, repo_name, branch, is_default_branch, note_content}` on stdin. Notes that arrive by fetch or pull from a remote don't run the hook, so each machine reports only its own notes (see Hook verification and Trade-offs).
- **`oceans hook claude`** is a Claude Code hook. At session start and end it sends the session id, repo URL, branch, HEAD and harness version. It sends no prompt text.
- **OMP support:** git-ai has no OMP preset ([git-ai#1626](https://github.com/git-ai-project/git-ai/issues/1626)). `oceans hook install --omp` installs an OMP extension that drives git-ai's `pi` preset before and after each `edit`, `apply_patch`, `write` and `bash` call, and reports session start and end through `oceans hook omp`, the OMP counterpart of `oceans hook claude`. Notes from it carry `agent_id.tool = "pi"`, so the note can't tell OMP from Pi. The join doesn't need it to (see Join). OMP sends no session id over Chat Completions or Responses, so the extension keeps `OCEANS_SESSION_ID` set to the main session's id, and the user's Oceans provider in `models.yml` sends it as the `session_id` header. OMP resolves a header value that names an environment variable on each request.
  - `session_id` is the header the `oh_my_pi` adapter already reads, and the Claude Code adapter ignores it. OMP's Messages requests carry a Claude Code user agent, so they keep their own `x-claude-code-session-id` without a conflict.
  - The canonical `x-oceans-session-id` was rejected for OMP. With routing affinity, a value that disagrees with `x-claude-code-session-id` fails the request, and an OMP subagent's Messages requests would always disagree.
  > Superseded by [Amendment: request persistence and journal identity](#amendment-2026-11-15-request-persistence-and-journal-identity): explicit request-to-journal mappings replace the environment bridge. Remove `session_id: OCEANS_SESSION_ID` from `models.yml` after upgrading the extension and restart OMP.
- **Merged pull requests:** `oceans hook prs` runs the local `gh api graphql` and sends each merged PR with its commit SHAs, subjects and author emails. It fetches pages of at most 49 PRs, so the nested commit and author selections stay below GitHub's 500,000-node query limit. `--limit` sets the total across pages. A failed request reports gh's stderr and queues no partial result. It runs at agent session start, at most once an hour per repository, and on demand. Squash and rebase merges create commits on GitHub that have no notes, so a PR is joined to the notes of its original commits by SHA. The gateway needs no GitHub token.
- **Cost and requests per change:** a commit gets the requests of each contributing session that happened after that session's previous commit and at or before this commit. Requests after a session's last commit count as uncommitted work. Totals therefore add up without double counting, and request counts stay whole. Costs come from `usage_cost_events`, so unpriced requests are counted separately and never treated as free.
- **Joining by email:** author emails in a note (`sessions.*.human_author`, `prompts.*.human_author`, `humans.*.author`) and PR commit author emails are matched to Oceans users by normalised email. The admin Attribution page groups activity by that user, and falls back to the key owner when a commit carries no email.
  > Superseded in part by [Amendment: key owner as note author](#amendment-2026-11-15-key-owner-as-note-author): a note sent with a user's key now names that user, and emails can also match a user's `additional_attribution_emails`.
- Both post to a new authenticated ingestion endpoint under `/api/v1` with an Oceans API key owned by the same user or service account whose keys send the matching gateway traffic. API keys have only these two owner kinds, and `usage_ownership_scope_key` ([`budget_scopes.rs`](../../crates/gateway-service/src/budget_scopes.rs)) maps them to `user:{id}` and `service_account:{id}`. There is no team ownership scope. The endpoint is Oceans' own contract, not a Git AI-compatible one.
- A new `attribution:write` API key scope grants only the ingestion endpoint and the ownership lookup, `GET /api/v1/me/ownership`, which the sender needs before it can deliver (see Owner binding). A full-access key also works.
- The gateway parses notes against the v3 spec and stores totals per commit and per session. It stores no file paths and no line ranges.

## Implementation

- **Hook budget:** git-ai waits 3 s for hook commands, then detaches them ([`git_ai_hooks.rs`](https://github.com/git-ai-project/git-ai/blob/main/src/authorship/git_ai_hooks.rs)). git-ai's background service writes the notes, so even a slow hook doesn't delay `git commit`. `oceans hook notes` still writes the batch to a local queue and returns, so a service restart or a network failure can't lose it. Delivery happens in the background, with retries.
- **Configuration:** the CLI reads `OCEANS_URL` and `OCEANS_API_KEY` from the environment today. The notes hook runs under git-ai's background service and the Claude hook under Claude Code, neither of which reliably has the user's shell environment. Putting the key in git-ai's hook command would store it in `~/.git-ai/config.json`, which other tools read. The CLI therefore gains a config file, described in [CLI configuration and key resolution](#cli-configuration-and-key-resolution). git-ai reads its own config once per process, so installing the hook also means running `git-ai bg restart`.
- **Repo identity:** git-ai passes the default remote's URL unchanged, which can include credentials (`https://token@host/...`). The CLI strips userinfo and normalises the URL to `host/org/repo` before it leaves the machine, and the gateway rejects URLs that still have userinfo.
- **Parser:** the parser is a dependency-light module, roughly 200–400 lines, that accepts all three attestation key formats (legacy hex, `s_…::t_…`, `h_…`). It counts attested lines per session and per known human. Unattested added lines stay unknown, not human.
- **Identity storage:** on the gateway, raw identifiers from a note exist only in memory while the request is parsed. They are never stored, never logged and never included in error messages. That covers `agent_id.id`, `human_author`, `humans.*.author` and the commit author. What is stored:
  - **Sessions:** the scope-bound hash described under Join, plus `agent_id.tool` and `agent_id.model`.
  - **Humans:** an author email that matches an Oceans user's email is stored as that user's id. Any other email is stored only as `hash_lineage_candidate(ownership_scope_key, "git_ai_human", lowercased email)`, which lets an owner's contributors be counted apart without storing who they are. As with session ids, this is correlation evidence within one ownership scope, not identity.
    > Superseded in part by [Amendment: key owner as note author](#amendment-2026-11-15-key-owner-as-note-author): a note sent with a user's key stores that user's id, whatever its emails.
- **Local queue contents:** the local queue does hold raw notes, identifiers included, until they're delivered. The CLI can't hash them first, because matching an email to an Oceans user happens on the gateway. The queue is limited instead:
  - **Exposure:** the queue adds no new exposure, because the same notes already sit in the repository's `refs/notes/ai` on that machine.
  - **Permissions:** the queue and quarantine directories are created owner-only (`0700`, with `0600` files). The CLI refuses to queue into a directory that other users can read.
  - **Retention:** an entry is deleted once the gateway acknowledges it, and a quarantined entry after 30 days, or earlier through `oceans hook queue --discard`.
  - **Logging:** the CLI never logs note content or identifiers. Queue listings show the repository, commit and reason only.
- **Join:** passive ingestion never stores a raw external id. It stores `hash_lineage_candidate(ownership_scope_key, namespace, value)` ([`session_resolution.rs`](../../crates/gateway-service/src/agent_analysis/session_resolution.rs)) as `normalized_session_id`. The namespace is `oceans` when the value came from the canonical `x-oceans-session-id` header, and otherwise the harness adapter's namespace. Attribution ingestion applies that same function:
  - The ownership scope comes from the ingesting key, so the join only works when that key resolves to the same scope as the keys that sent the gateway traffic.
  - The value is `agent_id.id` for a commit session and the hook's session id for a hook-reported session. It is hashed under every namespace in `SESSION_HASH_NAMESPACES` (`claude_code`, `codex`, `opencode`, `pi`, `oh_my_pi`, `oceans`), and the session joins whichever hash has requests. The harness the gateway detected doesn't have to agree with the note's `agent_id.tool` or the hook's harness. This matters because OMP over the Messages API sends a Claude Code user agent and headers, and because git-ai's `pi` preset serves both Pi and OMP. `agent_id.tool` and the hook's harness are kept as display labels; the hook's harness is also part of the hook session's row key. A hook session with no commit yet therefore still has its requests counted as uncommitted work, whichever namespace they were recorded under.
  - Two hashes of one session can both have requests, for example an OMP session sending both headers. The report unions their requests by request id, so nothing is counted twice.
  - Attribution tables store only the hashes, never the raw id, which keeps the ADR's rule that external values are scope-bound correlation evidence. Session ids are high-entropy values inside one owner's scope, so a match under the wrong namespace isn't a practical risk.
- **Subagent lines:** lines written by a Claude Code subagent carry the subagent's transcript name (`agent-<hex>`) instead of a session id. They join on the request's hashed `execution_id`, which comes from `x-claude-code-agent-id`, once the header's raw format is confirmed. Until then they count as AI lines without a matching session. A match is correlation evidence, not identity, as in the [passive agent session analysis ADR](2026-07-21-passive-agent-session-analysis.md). Gateway data supplies the model and cost when the note says `unknown`.
  > Superseded in part by [Amendment: request persistence and journal identity](#amendment-2026-11-15-request-persistence-and-journal-identity): an exact request mapping or same-owner note evidence must establish the link. Parent execution IDs, account names, models and time windows are not aliases.
- **Replacement key:** rewrites (rebase, amend, cherry-pick) produce new notes for new SHAs. git-ai can also write a note again for an existing SHA, for example in its untraced-commit fixup or CI rewrites, so a commit can receive more than one note. Ingestion is keyed on `(ownership_scope_key, repository, commit_sha)`. The ownership scope comes from the ingesting key, so two owners reporting the same repository and commit keep separate totals and never overwrite each other. Replacing a commit's note also replaces that commit's per-session rows in the same transaction. Rows for sessions the new note no longer mentions are deleted, so commit totals and session totals always come from the same note.
- **Freshness:** git-ai's hook payload carries no revision, so the CLI adds one:
  - **Stamping:** when either hook queues an entry, it stamps it with a per-installation id, a sequence number that only increases on that installation, and an `observed_at` time. Both hooks share one queue directory, so sequence numbers are allocated under a lock:
    - **Allocation:** a hook takes an exclusive file lock on the queue, reads the counter, and writes the next value to a temporary file that it renames over the counter. It then writes the entry the same way, by temporary file and rename, and releases the lock.
    - **Crashes:** a crash between the counter update and the entry write only leaves a gap in the numbers, which is harmless because sequences only need to increase. A crash during a write leaves only a temporary file, which the sender deletes.
    - **Lock timeout:** a hook waits at most 1 s for the lock. If it times out, it writes the entry to an `unsequenced` folder, keeping the `observed_at` it captured. The sender numbers those entries under the lock before delivering them and marks them `late_sequenced`, because their sequence can be higher than that of a newer entry queued while the hook waited.
  - **Ordering:** the sender delivers queued entries oldest first. A network error, a `5xx` or a `429` stops delivery, which retries later with backoff, so a retry never overtakes a later entry. The gateway returns a result for each entry. An entry it rejects (see Bounds) is quarantined with the reason and never retried, so it can't block the queue.
  - **Same installation:** the gateway replaces a stored note only when the incoming note has a higher sequence. An equal or lower sequence is a duplicate or a stale retry and is acknowledged without being applied. A `late_sequenced` note must also have a later `observed_at` than the stored note, so an entry that timed out waiting for the lock can't replace a newer note. This relies on the clock of a single machine, and only for entries that timed out.
  - **Different installations:** the later `observed_at` wins. This is best-effort, because clocks on different machines can drift. It only matters when two machines under the same owner both write a note for the same commit.
- **Owner binding:** the session hash and the replacement key both come from the delivering key's ownership scope. Every queued entry, from `oceans hook notes` and from `oceans hook claude` alike, must therefore be delivered under the owner it was queued for:
  - **Learning the scope:** the sender resolves the key and computes its fingerprint, the first 16 hex characters of its SHA-256, which is never the key itself. It then looks up the key's ownership scope through `GET /api/v1/me/ownership`. It caches the result locally, keyed by gateway URL and fingerprint, and records it as the current scope. `oceans login` and `oceans hook install` do this lookup up front, so the cache exists before the first hook runs.
  - **Stamping:** both hooks stamp each queued entry with the current scope. This reads only a local file, so the hooks never resolve the key or use the network.
  - **Before delivery:** the sender resolves the current key and gets its scope from the cache or a new lookup. A rotated key gets a new fingerprint, so a stale cached scope is never reused. An entry is moved to a quarantine folder and not sent if its stamped scope differs from that scope, or if it has no stamped scope.
  - **Server check:** each batch also sends `expected_ownership_scope`. The gateway rejects the batch with `409` if that value doesn't match the authenticating key's scope, so a client bug can't store data under the wrong owner. A `409` means the key changed between the local check and the request. The sender quarantines the batch's entries with the reason "owner changed", keeps them for `--resend`, and doesn't retry them. It then checks the current key's scope again before sending anything else.
  - **Changing keys and cleanup:** a key rotated to the same owner keeps delivering. A key changed to another owner quarantines the previous owner's remaining entries instead of moving them across. `oceans hook queue` lists quarantined entries and why each was quarantined. `oceans hook queue --discard` deletes them, and `--resend` delivers them again once the original owner's key is configured. Quarantined entries are deleted after 30 days.
- **Bounds:** the limits sit well above this repository's notes (1,051 notes: median 867 B, 99th percentile 19 KB, largest 69 KB). Every boundary enforces them, so no single input can create unbounded work:
  - **Hook:** each hook reads at most 64 MiB of stdin and stops reading beyond that. A note over 1 MiB is written straight to quarantine as oversized. The rest of the batch is queued.
  - **Queue:** the queue holds at most 512 MiB per installation. When it's full, the hooks stop queuing and keep a count of dropped entries, which `oceans hook queue` reports.
  - **HTTP:** the sender splits the queue into requests of at most 100 entries and 2 MiB. The gateway rejects a larger request body with `413` before parsing it.
  - **Parser:** the parser rejects a note that exceeds any of these limits: 1 MiB in total, 10,000 file paths, 100,000 attestation lines, 10,000 ranges per line, line numbers up to 10,000,000, or 256 KiB of JSON metadata. The batch still returns `200`, and the rejected entry gets `status: "rejected"` with a reason code (`oversized_note`, `invalid_note`, `unsupported_schema`, `invalid_repository` or `invalid_entry`).
- **Storage:** paired LibSQL and PostgreSQL schemas, following existing store conventions.

### As built

- **Routes:** `POST /api/v1/attribution/entries` (2 MiB body limit) and `GET /api/v1/me/ownership` accept an `attribution:write` or full-access key. `GET /api/v1/admin/attribution/report?days=7|30|90&user_id=&repository=` serves the admin Attribution page under the existing `usage_costs` page grant. Platform admins see every owner; other users see only rows they own or authored, as on Usage Costs.
- **Storage:** migration V79 adds `attribution_commits`, `attribution_commit_sessions`, `attribution_commit_session_hashes` and `attribution_agent_session_hashes` (one row per session namespace hash), `attribution_agent_sessions`, `attribution_pull_requests` and `attribution_pull_request_commits`, plus an index on `agent_session_requests(normalized_session_id, occurred_at)` for the join.
- **Report:** the gateway reads bounded rows (at most 1,000 commits and pull requests, and a capped set of request facts) and computes request windows in Rust, in `gateway-service/src/attribution/report.rs`. The response flags any read that hit its cap.
- **Commit time:** a commit's time is the `first_observed_at` of its first note, which stays the same when the note is replaced.

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
- **File permissions:** `oceans` refuses to use a config file that other users can write to, because a `!` command in it effectively becomes code that runs on every commit.
- **`oceans login`:** this command writes the file with owner-only permissions. Use a key with only the `attribution:write` scope for the hooks, so a leaked file or command output can only submit attribution data and read its own ownership scope. The scope is in the [scoped API keys ADR](2026-10-17-scoped-api-keys.md) catalogue and is enforced through `authenticate_for_scope`, with route-coverage tests.

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

The Git AI hook's stdin payload carried the remote URL exactly as configured, including the `user:token@` part. This check didn't exercise the outbound Oceans request. That request must carry the URL with the userinfo removed, as described under Repo identity, and needs its own check once the CLI is implemented.

Rechecked on 2026-11-12 with git-ai 1.7.6: the hook runs with `~/.git-ai/internal/daemon` as its working directory and no `GIT_DIR`, so `oceans hook notes` can't read commit metadata from the repository. Session author emails arrive in the note as `Name <email>`.

## Session id verification

Checked on 2026-11-01 with Claude Code 2.1.309. No Oceans gateway records Claude Code traffic on the maintainer's machine, so each link was checked separately.

- **Notes and Claude Code:** this repository's notes contain 89 distinct Claude session ids. 66 are the names of local Claude Code transcripts. 20 are subagent ids (`agent-<hex>`), the name of a transcript under the parent session's `subagents/` folder. git-ai uses that name when the hook payload has no `session_id`. The remaining 3 have no local transcript.
- **Claude Code and the gateway header:** a `claude -p` run sent through a loopback proxy carried `x-claude-code-session-id: e72a8b10-c0ba-4895-8a5b-16cccdd0cca7`. That was also the hook's `session_id` and the transcript's name.
- **Gateway normalisation:** `client_session.rs` passes the header value through and only rejects empty, oversized or non-identifier values. Ingestion then hashes it with the ownership scope and harness before storing it. A note id therefore matches only after the same hash, as described under Join.
- **Not verified:** the values a subagent's requests send in `x-claude-code-agent-id` and `x-claude-code-parent-agent-id`. The run that would have spawned a subagent was refused by the API before any tokens were used. Subagent transcripts record `agentId` as bare hex, without the `agent-` prefix git-ai adds.

Rechecked on 2026-11-12 with a loopback capture server standing in for the gateway. The first end-to-end OMP run joined no sessions, so each harness was captured:

| Harness | User agent, and the harness the gateway detects | Session id sent | Extracted before this change |
|---|---|---|---|
| Claude Code 2.1.303 | `claude-cli/…`, `claude_cli` | `x-claude-code-session-id`, `metadata.user_id` | No. `claude_cli` had no adapter. |
| Codex exec 0.166 | `codex_exec/…`, not matched | `session-id`, `x-codex-turn-metadata` | No. The user agent wasn't matched. |
| OpenCode 2.2.1 | `opencode/…`, `opencode` | `x-session-id`, `x-opencode-session` | Yes |
| Pi 1.4, Responses | `pi/…`, `pi` | `session_id` | Yes |
| Pi 1.4, Chat Completions | `pi/…`, `pi` | None | No |
| OMP 18.18, Chat Completions and Responses | `omp/…`, `oh_my_pi` | None. Responses sends `prompt_cache_key`, which the gateway deliberately ignores. | No |
| OMP 18.18, Messages | `claude-cli/…`, `claude_cli` | `x-claude-code-session-id`, `metadata.user_id` | No |

The fixes: `claude_cli` uses the Claude Code adapter, `codex_` user-agent prefixes classify as Codex, and passive analysis reads `x-oceans-session-id` for every harness, as routing already did, so any client can name its session. OMP gets a `session_id` header through the extension and `models.yml`, as described under OMP support. Pi over Chat Completions still sends no session id.

## Why

- **Git AI already handles the hard part:** git-ai is about 175k lines of Rust, most of it moving attribution through rebase, squash, cherry-pick, stash and amend. Its documented `post_notes_updated` hook gives us the result of that work without depending on its internals.
- **No paid dependency:** the hook and the notes format work with free, local Git AI. The Analytics API and self-hosted platform both need a Git AI for Teams licence.
- **Joins with data Oceans already holds:** the hook runs on the machine that also sends gateway traffic, with a key from the same owner. The ownership scope and the scope-bound session hash then line up without mapping identities across systems.
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
- **Rewritten commits count their lines again:** after a rebase, both the old and new SHAs keep their notes, so the report counts their AI lines twice. Requests aren't double counted, because windows split each session's requests by time. The rewritten commit gets the requests made between the original commit and the rewrite. PR views count only commits that belong to a merged PR, so merged totals are unaffected.
- **OMP through the Pi preset:** the extension finds edited paths by parsing OMP's edit payloads (`path`, apply_patch and hashline headers). Files changed by other tools, such as `eval` or `ast_edit`, get no AI checkpoint, so git-ai records them as untracked or human.
- **OMP session header needs the extension:** OMP sends a header value's text as-is when it names an unset variable. If `models.yml` has the header but the extension isn't installed, every Chat Completions and Responses request carries `OCEANS_SESSION_ID`, and passive analysis counts all of that machine's OMP traffic on those APIs as one session. The install output and docs say to add the header only where the extension is installed.
- **OMP subagents:** subagents run in the main session's process and share its environment, so only the main session sets the variable. A subagent's Chat Completions and Responses requests count for the main session. Its lines carry its own session id, so they count as AI lines without a matching session, while its cost lands in the main session's windows.
  > The two OMP limitations above are superseded by [Amendment: request persistence and journal identity](#amendment-2026-11-15-request-persistence-and-journal-identity). They describe the original environment bridge, not the current extension.
- **The upgrade locks request writes while an index builds:** V79 adds an index on `agent_session_requests(normalized_session_id, occurred_at)`. The migration runner applies each migration in a transaction, so PostgreSQL can't build it `CONCURRENTLY`, and writes to that table wait until the build ends. V79 allows 15 minutes instead of the usual 60 s, so a large table doesn't roll the migration back. Operators with a large table should upgrade at a quiet time.
- **Authorship is a claim:** an author email comes from the commit, which anyone can set. A user's report also shows commits that name them as author, so a key holder can credit their own commit to another user. That user then sees the commit with the sender's requests and cost. No other owner's requests reach it, because session hashes are bound to the sender's ownership scope.
  > Superseded in part by [Amendment: key owner as note author](#amendment-2026-11-15-key-owner-as-note-author): a user's own notes can no longer be credited to someone else.
- **Report lookups read each session's whole history:** finding a request's commit reads every earlier commit and request of its sessions, not just the window's. A tighter bound would need each session's last commit before the window, per hash. Request lookups stop at 200,000 rows and mark the report incomplete.
- **Long sessions have a limit:** a hook-reported session open across the window counts its requests in the window as uncommitted only if it started within 30 days before the window.

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

## Amendment 2026-11-15: key owner as note author

Changed in [#654](https://github.com/ahstn/oceans-llm/pull/654), after the first deployment.

### Decision

- **The key owner is the note author:** git-ai runs the notes hook on the machine that made the commit. So a note sent with a user's key names that user as the author of the commit and its sessions, whatever email their Git config uses. Author emails still decide for notes sent with a service account key, and for PR commits, which `gh` fetches and which can be anyone's.
- **Additional attribution emails:** `users[].additional_attribution_emails` in the gateway config lists more emails for a config user, such as a personal Git email or a GitHub `noreply` address. An email is matched to an Oceans user by normalised email first, then by this list. A note from a user's key stores that user's id. A matched email is stored as the user's id, and any other email is stored only as its scope-bound hash, as before.

### Why

Notes matched their author emails even when a user's key sent them. A developer whose Git email differed from their Oceans email then showed as an unmatched author on every commit, although the key already proved who sent the note.

### As built

- **Ingestion:** the gateway names the key's owner as the author of a note from a user's key. Migration V80 rewrites the stored notes of user-owned keys to match. Stored pull requests keep their email hashes until `oceans hook prs` reports them again, because the raw emails were never stored.
- **Email list:** the gateway holds `additional_attribution_emails` in memory and checks it after the users table, so it needs no migration. Config validation rejects an email listed twice, one that is already a user's email or the bootstrap admin email, and one that author matching can't accept.

### Trade-offs

- **Authorship is still a claim for PRs and service accounts:** a PR commit's author email, and the emails in a service account's notes, come from the commit. So any key holder can credit a pull request to another user, and a service account key holder can credit a note's commit, with that account's requests and cost, to another user. No other owner's requests reach it, because session hashes are bound to the sender's ownership scope. A pull request counts the lines and cost of only the notes its viewer can already see, so a user's notes don't follow a pull request credited to someone else.
- **Notes credit the key, not the committer:** a note for a commit someone else authored, such as one a rebase rewrote, still counts for the user whose machine sent it.

## Amendment 2026-11-15: request persistence and journal identity

### Findings

- The first deployed snapshot had 110 analysis request rows: 108 October diagnostic requests and two November OMP smoke requests. It did not measure recent coding traffic coverage. A later snapshot had 121 distinct recent billed requests but only three matching request logs. Sampled Responses requests had a `complete` trace and a usage row without a request-log or analysis row.
- The shared stream finalizer marked itself finished before it awaited usage accounting and request logging. Dropping the response body during those awaits could cancel persistence while the drop handler skipped its fallback. A terminal provider event is not a promise that the client will read to EOF.
- The OMP extension and its configured session header were present. A fresh request's session hash matched a hook-reported session. Missing headers were therefore not established as the main cause of the deployed gap.
- OMP 18.20.2 separates journal identity from provider identity. `/fresh`, reset and provider-session overrides can change the provider ID without changing the journal. The old environment bridge also named the main journal on subagent requests. Pi and OpenCode have newer session-header contracts that the gateway did not fully recognize.

### Decision

- **Owned stream finalization:** usage accounting and request-log persistence must finish independently of the HTTP body's lifetime. One owner handles success, failure or cancellation, without charging or logging the same request twice. Responses terminal events must not depend on a later EOF; Chat finish reasons must still allow trailing usage.
- **Explicit request mapping:** an OMP response hook reports the gateway-only `x-oceans-request-id` with its context's journal ID. A `request_session` attribution entry carries `request_id` and `session_id` through the existing ownership-bound queue. It does not use a provider's request ID or guess a journal from its provider session.
- **Bounded process work:** OMP detaches one `oceans hook request` process per response so the journal mapping reaches the durable queue without waiting on delivery. That process runs the existing sender itself instead of starting a second process. The sender lock coalesces concurrent delivery before key resolution or network access. Each mapping still requires its own queue write; no new daemon or in-memory-only buffer is introduced.
- **Gateway request IDs:** `x-oceans-request-id` and the stored request use one server-generated UUID. The caller's `x-request-id` remains a trace-correlation header, not an attribution identity. OpenCode conversation headers are validated before routing flattens repeated raw headers.
- **Scoped storage:** the gateway stores the journal ID only as an owner-scoped hash in the `oceans` namespace. A mapping can arrive before or after the request. Attribution prefers that explicit mapping over the native provider session when it reads request facts; native session correlation remains available without a mapping. Ownership boundaries, request deduplication and commit time windows still apply.
- **No native session required:** an explicit mapping can join a stored request log by gateway request ID and its recorded owner even when passive analysis created no session row. If an analysis row exists, its millisecond request time is retained; otherwise the stored log's second-resolution time is used. This matters for OMP custom transports that send no native session ID. Usage still joins only within the same owner scope.
- **Distinct meanings:** conversation IDs, provider session IDs, cache affinity and execution/thread IDs are not interchangeable. Header adapters recognize evidenced client formats and retain conflict checks for fields that claim the same identity.
- **Codex note evidence:** a request's stored execution hash can identify a thread or, on older input paths, a turn. Attribution prefers that exact hash only when the same owner's stored commit notes attest it. Otherwise it keeps the native session fallback. The evidence lookup is independent of the current report window and lookup chunk; a narrower query must not send a child's already-matched request back to its parent. Parent execution IDs are never aliases.
- **Client formats:** source checks used [OpenCode 2.3.0](https://github.com/anomalyco/opencode/blob/d432a06b9c07bab0be4e8d642173b1cb14137f11/packages/core/src/session/model-request.ts#L327-L342), [Pi 1.5.2](https://github.com/earendil-works/pi/blob/4f95fbe2f117d288610e937889ad03575777841c/packages/ai/src/api/openai-responses.ts#L315-L323) and [OMP 18.20.2](https://github.com/can1357/oh-my-pi/blob/7c49c11aa5bbaf3f418406583096168ad5b8c6dc/packages/ai/src/providers/anthropic-identity.ts#L5-L43). OpenCode conversation IDs take precedence over cache affinity. Pi's historical `session_id` remains readable alongside its current provider-specific session headers. Claude-compatible body fallback reads only a recognized explicit session component, never the whole account/device value.

### Verification and limits

- Regression proofs cover body cancellation during finalization, exact-once accounting and logging, mapping arrival order, different provider and journal IDs, subagent contexts, and cross-owner isolation. Client contract checks must assert the IDs used for attribution, not only a synthetic request tag.
- The response hook can report only requests whose gateway response headers reach it. It cannot recover requests cancelled before response headers, or paths that do not emit that hook. Native provider identity remains unchanged.
- This is not a historical backfill. Retained explicit IDs or exact request receipts can support a separate audited repair. Billing rows, timestamps and model names alone cannot establish an exact session. The analysis recompute command does not recreate missing request records, and PR resync does not repair request-side data.
- Migration V81 adds the scoped request mapping table and an index on existing request execution hashes and times. PostgreSQL applies it in a transaction, so the index build can block request writes. As with V79, the migration permits up to 15 minutes for that build. Upgrade at a quiet time on a large database; this fix does not deploy or migrate the running instance by itself.

## Amendment 2026-11-15: report insights for the Attribution tabs

### Decision

- **The report computes the tab aggregations.** Next to its tables, `GET /api/v1/admin/attribution/report` returns:
  - `daily` buckets: lines, commits and merges, with committed requests on their commit's day and uncommitted requests on the day they happened.
  - `session_outcomes`: agent sessions by the furthest stage their work reached, which is merged, default branch, not merged yet or no commit.
  - `harness_models` rows.
  - Per pull request: its matched commits, `first_commit_at`, `lead_time_seconds`, `is_revert` and agent tools.
  - A `summary` for the window and one for the window of equal length before it.
- **Sessions join by hash.** A hook-reported session and a note's session are one session when they share a session hash. Requests count once per request id, as elsewhere in the report.
- **Previous window:** only when a request sets `compare=true` (the Impact and My work tabs), the handler builds the preceding window's summary in parallel through `build_attribution_summary`. It reads what the full report reads, because the summary depends on the request allocation, but aggregates only what the summary needs: no changes, users, models, daily buckets, harness rows or per-commit details. Each read stays bounded by the same 1,000-row caps.
- **Report shapes:** `shape=harness` builds only the totals, coverage, session outcomes and harness × model rows, for the Agent Harnesses page; the default `full` shape builds everything the tabs read. Every shape reads the same rows, because each depends on the request allocation.
- **Revert proxy:** a merged pull request whose title starts `Revert ` or `revert:` counts as a revert. It stands in for change failures until incidents reach Oceans.
  > Superseded by [Amendment 2026-11-16: pull request lifecycle and revert links](#amendment-2026-11-16-pull-request-lifecycle-and-revert-links): a revert counts only with a revert commit line, GitHub's revert button or a matching change. A title alone is a possible revert and doesn't count, and only the hook judges titles.
- **Tabs are config:** `attribution.pages` turns each tab on or off, and `AuthSessionCapabilitiesView.attribution_tabs` lists the tabs that are on. Other derivations, such as repository rollups, AI-share bands and the punch card, run in the browser over the report's capped rows.

### Why

Every tab needs the same joined rows. Daily buckets and session outcomes have to see the request allocation, which only exists inside the report builder. Computing them there avoids a second request-fact read.

### Trade-offs

- **No open or closed PRs:** pull requests come only from `gh pr list --state merged`, so session outcomes can't separate "in review" from "abandoned". Work that hasn't merged stays in one stage. (The 2026-11-16 amendment adds open and closed pull requests; the session outcome stages still don't use them.)
- **Missing data stays visible:** line survival, review times, deploys and incidents are missing. The UI keeps those sections with a "Needs new data" pill. [Attribution data gaps](../internal/2026-11-15-attribution-data-gaps.md) lists the gaps and how to collect each one.

## Amendment 2026-11-16: pull request lifecycle and revert links

### Decision

`oceans hook prs` reads open, closed and merged pull requests, most recently updated first, instead of merged ones only. It sends each one's state, draft flag, opened and closed times, first human review time, review, approval, changes-requested and comment counts, and lines added, deleted and files changed. It also sends a revert link: which pull request or commits this one reverts, and the evidence. The hook finds the link from the strongest signal available:

1. `revert_commit`: a pull request commit carries git's `This reverts commit <sha>` line.
2. `github_revert`: GitHub's revert button (`Reverts owner/repo#N` in the description, or a `revert-N-…` branch), counted only with a revert title.
3. `change_match`: the pull request's whole change is the exact inverse of an earlier merged pull request's, by a fingerprint of the added and removed lines. The change runs from the merge or squash commit's first parent, or, for a rebase merge, from before the first rebased commit: a single-parent merge commit whose last `N` first-parent commits carry the pull request's `N` subjects in order is taken as a rebase.
4. `title`: a revert title alone. A possible revert, not counted as one.

The gateway stores the lifecycle as `state`, `opened_at` and `closed_at` columns plus a `details` JSON object (migration V82, which makes `merged_at` nullable). The report's commit, spend and delivery figures still use pull requests merged in the window. A new lifecycle summary counts opened, closed-unmerged, open and draft pull requests, merges without review (no human review before the merge), review wait, confirmed and possible reverts, and time to revert. The report resolves each link by number, or by a reverted commit listed in, or merging, another pull request.

### Why

- **The revert line is machine-written.** Studies of reverted commits, and change-failure tools such as Swarmia's, key on git's `This reverts commit` line and GitHub's revert-button markers. Titles are rewritten often enough that a title match both misses reverts and flags non-reverts.
- **The change match catches reworded reverts.** A revert's change is the inverse of the original's whatever its message says. The fingerprint leaves out line numbers but keeps three context lines on each side of an edit and the hunk's function label, so unrelated edits elsewhere between the two don't break the match, while the same lines changed in another function or method don't match. A false match still needs identical lines for three lines either side within one file. It needs nothing beyond the local clone the hook already runs in.
- **Counts, not identities.** Review data reduces to counts and times on the developer's machine. Reviewer logins, descriptions, commit messages and fingerprints never leave it, which keeps the ADR's rule that raw external identifiers are not stored.

### Trade-offs

- **Draft history:** the CLI reports only the current draft state, so review wait runs from opened, not from ready for review. A GitHub App's `ready_for_review` events would fix that.
- **Change match needs local history:** a merge commit missing from the clone is skipped, as is a single-parent merge of a pull request with 100 or more commits, since `gh` lists only the first 100 and a rebase can't be ruled out; and a revert of a pull request the hook never fingerprinted only matches by its other signals. The fingerprint cache keeps the newest 5,000 merge commits per repository. A sync fingerprints at most 200 new merge commits, for at most 30 seconds, most recently updated first, so a first sync over a long history stays quick and the backlog fills over the next few syncs; until then a reworded revert of an unfingerprinted pull request isn't matched.
- **Partial reverts and fix-forwards aren't detected:** only an exact inverse matches. Incident links are the real answer.
- **Older rows show no revert:** pull requests stored before this change have no link, and the gateway doesn't judge titles itself, so they show no revert, confirmed or possible, until the hook re-sends them. One title rule, in the hook, means ingestion and the report can't classify the same title differently.
- **Bot reviews are excluded by account type:** GitHub's `Bot` actors are left out, but a bot that reviews from a user account counts as a person.
- **A 90-day lookback for revert links:** when a confirmed revert merged in the window names a pull request the window's read doesn't hold, the report also reads the 90 days before the window, so the revert resolves an older original and yields a time to revert. Most windows have no such revert and skip that read, which carries full commit lists. Those older rows only resolve links; window figures count activity in the window. A revert of a change older than that resolves only by number, without a time.
- **Binary files match by blob ids:** a binary change has no lines to compare, so the fingerprint uses its from and to blob ids, which keep the change's direction. File mode changes count with their old and new modes, and git's missing-final-newline marker counts with its line, so restoring a file's text without its executable bit or final newline isn't an exact revert.
- **More than 100 reviews:** the hook reads the first 100 and sends the counts as unknown when there are more. The first review time is still exact, since reviews come oldest first.
- **Activity is the forge's, not the sync's:** the hook sends GitHub's `updatedAt`, stored as `last_activity_at`, which orders the report read and decides whether a pull request changed in the window. A routine re-sync of an untouched pull request doesn't count. The gateway fills `last_activity_at` from the hook's value, else the merge, close or open time, and never from when it was seen; a write with none of them keeps the stored value; rows stored before V82 keep it empty, and the read falls back to their merge time rather than rewriting the table in the migration.
- **Context costs a few matches:** an edit right next to the original change, made before the revert, changes the context line and breaks the match; the revert then counts by its other signals or as possible. A false confirmed revert would skew change failure and recovery, so a missed match is the safer error. The fingerprint cache file is versioned with the fingerprint format.
- **Exact lines:** the change fingerprint keeps whitespace, since a revert reproduces the original lines exactly and whitespace inside a string is content.
- **Copies from several owners:** the author's copy keeps attribution, and the state, merge, commits and lifecycle come from the most recently observed copy, so a teammate's sync after the merge isn't overridden by the author's earlier one.

## Follow-ups

- Capture `x-claude-code-agent-id` and `x-claude-code-parent-agent-id` from a Claude Code run that spawns a subagent, and confirm how they relate to git-ai's `agent-<hex>` ids. In this repository, 20 of 89 Claude session ids in notes are subagent ids.
- Check whether Codex session ids in notes match the gateway's Codex `session-id` values.
- Confirm the join on a deployment where a gateway records Claude Code traffic for the same commits.
- Server-side clone backfill as an optional source for history and for machines without the hook. Admin-entered clone URLs need allowlisted SCM hosts and the SSRF rules in [SSRF-safe outbound fetch](2026-10-17-ssrf-safe-outbound-fetch.md).
- Optionally serve a read-only subset of the Analytics API shape so external tools, such as Git AI's DevLake plugin, can read Oceans. Check Git AI's terms before copying their spec.
- Ask git-ai for a native OMP preset ([git-ai#1626](https://github.com/git-ai-project/git-ai/issues/1626)) so notes say `oh_my_pi` directly.
- Ask OMP for a request-options hook carrying both journal attribution identity and provider identity, including maintenance and native-judge requests, so those paths can report exact associations too.
- Bound the report's commit-time and request lookups per session hash, once real reports show long-lived sessions.
