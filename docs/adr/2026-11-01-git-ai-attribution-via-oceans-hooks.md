# ADR Git AI attribution through `oceans hook`

- Date: 2026-11-01
- Status: Accepted

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
- **Merged pull requests:** `oceans hook prs` runs the local `gh pr list --state merged` and sends each PR with its commit SHAs, subjects and author emails. It runs at agent session start, at most once an hour per repository, and on demand. Squash and rebase merges create commits on GitHub that have no notes, so a PR is joined to the notes of its original commits by SHA. The gateway needs no GitHub token.
- **Cost and requests per change:** a commit gets the requests of each contributing session that happened after that session's previous commit and at or before this commit. Requests after a session's last commit count as uncommitted work. Totals therefore add up without double counting, and request counts stay whole. Costs come from `usage_cost_events`, so unpriced requests are counted separately and never treated as free.
- **Joining by email:** author emails in a note (`sessions.*.human_author`, `prompts.*.human_author`, `humans.*.author`) and PR commit author emails are matched to Oceans users by normalised email. The admin Attribution page groups activity by that user, and falls back to the key owner when a commit carries no email.
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
- **Storage:** migration V78 adds `attribution_commits`, `attribution_commit_sessions`, `attribution_commit_session_hashes` and `attribution_agent_session_hashes` (one row per session namespace hash), `attribution_agent_sessions`, `attribution_pull_requests` and `attribution_pull_request_commits`, plus an index on `agent_session_requests(normalized_session_id, occurred_at)` for the join.
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
- **The upgrade locks request writes while an index builds:** V78 adds an index on `agent_session_requests(normalized_session_id, occurred_at)`. The migration runner applies each migration in a transaction, so PostgreSQL can't build it `CONCURRENTLY`, and writes to that table wait until the build ends. V78 allows 15 minutes instead of the usual 60 s, so a large table doesn't roll the migration back. Operators with a large table should upgrade at a quiet time.
- **Authorship is a claim:** an author email comes from the commit, which anyone can set. A user's report also shows commits that name them as author, so a key holder can credit their own commit to another user. That user then sees the commit with the sender's requests and cost. No other owner's requests reach it, because session hashes are bound to the sender's ownership scope.
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

## Follow-ups

- Capture `x-claude-code-agent-id` and `x-claude-code-parent-agent-id` from a Claude Code run that spawns a subagent, and confirm how they relate to git-ai's `agent-<hex>` ids. In this repository, 20 of 89 Claude session ids in notes are subagent ids.
- Check whether Codex session ids in notes match the gateway's Codex `session-id` values.
- Confirm the join on a deployment where a gateway records Claude Code traffic for the same commits.
- Server-side clone backfill as an optional source for history and for machines without the hook. Admin-entered clone URLs need allowlisted SCM hosts and the SSRF rules in [SSRF-safe outbound fetch](2026-10-17-ssrf-safe-outbound-fetch.md).
- Optionally serve a read-only subset of the Analytics API shape so external tools, such as Git AI's DevLake plugin, can read Oceans. Check Git AI's terms before copying their spec.
- Ask git-ai for a native OMP preset ([git-ai#1626](https://github.com/git-ai-project/git-ai/issues/1626)) so notes say `oh_my_pi` directly.
- Ask OMP to send a session id to custom providers, so the `models.yml` header and its failure mode can go.
- Bound the report's commit-time and request lookups per session hash, once real reports show long-lived sessions.
