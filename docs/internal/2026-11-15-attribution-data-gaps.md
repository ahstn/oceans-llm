# Attribution data gaps

- Date: 2026-11-15
- Status: research notes for maintainers. Not a decision and not a contract.
- Related: [Git AI attribution ADR](../adr/2026-11-01-git-ai-attribution-via-oceans-hooks.md), [visualisation concepts](../design-proposals/attribution/README.md)

## Why

The Attribution page is being rebuilt into tabs: Impact (with the session-outcome Sankey), Delivery, Pull requests, Repositories, Durability and My work. The Agent Harnesses page gains a harness × model table. Several widgets on these tabs carry a "Needs new data" pill, because Oceans only stores what the [ADR](../adr/2026-11-01-git-ai-attribution-via-oceans-hooks.md) describes: per-commit AI and human line totals from Git AI notes, harness session start and end events, and merged PRs from `oceans hook prs`. This note lists every missing data point, what it would unlock, what we can show as a proxy today, and the ways we could collect it.

Any new collection must keep the ADR's privacy rules. Raw external identifiers (session ids, emails, logins) are never stored. File paths and line ranges are never stored. Records are bound to the ownership scope of the key that sent them.

## What we store today

- **Commits** ([`attribution.rs`](../../crates/gateway-core/src/attribution.rs), `AttributionCommitWrite`): repository, SHA, branch, default-branch flag, author (user id or scope-bound email hash), `ai_lines`, `human_lines`, and per-session AI lines with tool, model and session hashes. The parser ([`note.rs`](../../crates/gateway-service/src/attribution/note.rs)) reads file paths and line ranges from the note's attestation section, counts them, then drops them.
- **Sessions** (`AttributionSessionEventWrite`): harness, session hashes, repository, branch, start and end HEAD, harness version, start and end times.
- **Pull requests** (`AttributionPullRequestWrite`): number, title, URL, base and head ref, merge commit SHA, `merged_at` (required), author, and each commit's SHA, subject, `authored_at` and author. [`prs.rs`](../../crates/oceans-cli/src/attribution/prs.rs) asks `gh pr list --state merged` for `number,title,url,mergedAt,baseRefName,headRefName,mergeCommit,commits`.
- **Gateway requests**: per-request facts from passive session analysis, including `message_count`, prompt bytes and tool counts ([`agent-session-analysis`](../../crates/agent-session-analysis/src/lib.rs), [`session_resolution.rs`](../../crates/gateway-service/src/agent_analysis/session_resolution.rs)), plus priced cost from `usage_cost_events`.

## Summary

| # | Gap | Unlocks | Proxy today | Best avenue | Effort |
|---|---|---|---|---|---|
| 1 | PR lifecycle: opened, draft, reviews, comments, open and closed-unmerged PRs | Review wait, merged without review, abandon rate, PR state badges, "in review" and "closed" Sankey stages | First commit `authored_at` as a stand-in for opened; nothing for reviews | More `gh` fields in `oceans hook prs`, later a GitHub App | S |
| 2 | Revert and fix-forward detection | Change fail rate proxy, "merged PRs vs reverts" counterweight, revert badge | Title match on merged PRs | Parse `This reverts commit` client-side in `oceans hook prs` | S |
| 3 | Deploy and release events | Deployment frequency, lead time to production, failed deployment recovery time, deployment rework rate | Merge to default branch as "deploy" | GitHub Deployments via the App, or a CI step | M |
| 4 | Incidents linked to changes | Change failure rate, recovery time, session failure rate | Reverts as failures | Incident tool webhooks plus a service-to-repository map | M-L |
| 5 | Deleted lines and file counts per commit | Refactor and moved rate, true PR size, net change | AI plus human lines as size; distinct files per note | Count files from notes on the gateway; numstat at session end | S-M |
| 6 | Generated lines per session, including discarded work | Generated : committed ratio, AI code journey funnel | Uncommitted spend from requests after a session's last commit | Edit-tool line counts from the Claude and OMP hooks | M |
| 7 | Line survival and churn at 14, 30 and 90 days | Durability tab, churn, cost per durable line, readiness quadrant y-axis | None worth showing | Scheduled CI job that runs blame and posts counts | L |
| 8 | AI lines rewritten in review | Review rework share, PR ledger "rewritten" column | Commits authored after the first review | Needs 1 and 7 | M |
| 9 | Human steering (user turns per session) | Autonomy and steering on Repositories, harness × model table, My work | `message_count` growth per session | Derive from logged requests; `UserPromptSubmit` hook as a cross-check | S |
| 10 | Notes from other machines and CI | Coverage meters, team totals that do not depend on everyone installing the hook | Share of PR commits with a note | Service-account backfill job | M |
| 11 | Survey and satisfaction (optional) | SPACE and DX context next to the numbers | None | In-app pulse with aggregation thresholds | M |

## The gaps

### 1. PR lifecycle

**Missing:** `opened_at`, draft state and `ready_for_review` time, first review time, review count, approvals, changes requested, distinct reviewers, comment count, and every PR that is open or closed without merging. `AttributionPullRequestWrite.merged_at` is required, so the record cannot hold an unmerged PR yet.

**Unlocks:** review wait (opened or ready to first review) on Delivery, merged without review rate, abandon rate (closed unmerged over all closed), state badges on the Pull requests ledger, and the "in review" and "closed" stages of the Impact Sankey. Without them the Sankey goes straight from "PR" to "merged" and drops everything else into "uncommitted".

**Proxy today:** the earliest commit `authored_at` in a PR stands in for opened time. It overstates lead time for long-lived branches and says nothing about review.

**Avenues:**

- **Extend `oceans hook prs`** (S). `gh pr list --state all` with `createdAt,closedAt,isDraft,state,reviews,comments,additions,deletions,changedFiles` returns everything above. Send counts and timestamps only: first review time, approval count, changes-requested count, reviewer count and comment count. Reviewer logins are raw identifiers and must stay on the machine, or be matched to Oceans users there. Do not request `files`, which carries paths. Trade-offs: `reviews` and `comments` make the GraphQL query heavier, so lower `--limit` or page; draft history is only the current state, not when it changed; data is as fresh as the hourly run.
- **GitHub App** (M-L). `pull_request` (`opened`, `ready_for_review`, `converted_to_draft`, `closed`, `reopened`, `synchronize`) and `pull_request_review` events give exact times and draft transitions. See [GitHub App](#github-app).

### 2. Revert and fix-forward detection

**Missing:** which PRs or commits undo or repair earlier ones. Git AI writes no note for `git revert` (ADR, Hook verification), so a revert is invisible in notes.

**Unlocks:** the change fail rate proxy by AI-share band on Delivery, the "merged PRs vs reverts" counterweight on Impact, and a revert badge with a link to the original PR on the ledger.

**Proxy today:** match merged PR titles against `Revert "…"`. This misses reverts with edited titles and cannot find the original PR unless the title is unchanged.

**Avenues:**

- **Parse in `oceans hook prs`** (S). `gh` already returns each commit's `messageBody`. The CLI can extract the SHA from `This reverts commit <sha>` and the PR number from GitHub's `Reverts org/repo#123` body, and send only `reverts_commit_sha` and `reverts_pr_number`. GitHub's revert button also names the branch `revert-<n>-<branch>`. No new privacy surface.
- **Fix-forward** is harder. Candidates: a PR labelled `hotfix` or titled `fix:` that merges within N days of another PR and shares a changed file. Sharing a file needs file identity, which we do not store (see gap 5). A safer first step is labels plus a short time window per repository, shown as a proxy.
- **Incident links** (gap 4) give the real answer when teams record the causing change.

### 3. Deploy and release events

**Missing:** when a SHA reached an environment, whether that deploy succeeded, and when a failed deploy was fixed.

**Unlocks:** the Delivery DORA tiles: deployment frequency, change lead time to production (merge to deploy, not merge alone), failed deployment recovery time and deployment rework rate (deploys that were unplanned fixes).

**Proxy today:** treat each merge to the default branch as a deploy. Fine for trunk-and-continuous-deploy teams, wrong for release trains.

**Avenues:**

- **GitHub Deployments API** through the App: `deployment` and `deployment_status` events carry environment, SHA, state and time. Only works when the CD system writes GitHub deployments.
- **CI step** (S per repository): a job after deploy posts `{repository, environment, sha, status, finished_at}` and the commit range since the last deploy (`git rev-list prev..sha`). The range lets the gateway map deploys to PRs without holding a commit graph.
- **CD systems:** Argo CD notifications and Flux's notification controller both support generic webhooks with a revision and sync status. `release` events cover teams that ship tags.
- **Mapping deploys to PRs** is the hard part in every case. Without the commit range, the gateway can only assume a PR shipped in the first deploy after its merge on that branch.

### 4. Incidents linked to changes

**Missing:** incidents, their start and resolve times, the service affected, and the change that caused them.

**Unlocks:** true change failure rate and recovery time on Delivery, and a session failure rate (share of sessions whose shipped code is later tied to an incident or revert) on Impact and the harness × model table.

**Proxy today:** reverts (gap 2) as failures. Recovery time from revert merge minus original merge.

**Avenues:** webhooks from PagerDuty, Opsgenie, incident.io, Sentry, Datadog, Grafana alerting, Prometheus Alertmanager and status pages. See [Monitoring and incident tools](#monitoring-and-incident-tools). Every option needs a service-to-repository map that admins maintain, and a rule for linking an incident to a change: an explicit field or label naming a PR, Sentry's suspect commits for a release, or "the last deploy of that service before the incident started". The last rule is cheap and often wrong.

### 5. Deleted lines and file counts per commit

**Missing:** deleted lines, files changed, renamed and moved lines. Notes attest added lines only.

**Unlocks:** refactor and moved rate on Repositories, true PR size and the batch-size histogram on Delivery, net change on the ledger.

**Proxy today:** `ai_lines + human_lines` as size. The gateway can also count distinct files in a note's attestation section before it drops the paths, with no new ingestion (S).

**Avenues:**

- **Notes hook numstat** is blocked. git-ai runs the hook from its daemon directory with no `GIT_DIR` (ADR, recheck on 2026-11-12), so `oceans hook notes` cannot run `git diff --numstat`.
- **Session-end hook** (S-M). `oceans hook claude` and `oceans hook omp` run in the repository. At session end they can run `git log --numstat --format=%H start_head..end_head` and send per-commit totals: added, deleted, files, renames. Moved lines need `git diff --color-moved` parsing, which is more work and slower. Misses commits made outside a session.
- **`gh` PR fields** (S): `additions`, `deletions` and `changedFiles` per PR, already reachable in gap 1's query. PR-level only.
- **Directory buckets.** A per-directory breakdown would need hashed paths. Top-level directory names are low-entropy (`src`, `tests`, `docs`), so a plain hash is reversible by guessing. An HMAC with a gateway secret only moves the guessing to the gateway. Keep counts only unless a concrete widget needs buckets, and make buckets opt-in per repository.

### 6. Generated lines per session, including discarded work

**Missing:** lines an agent wrote that were later changed, reverted or never committed.

**Unlocks:** generated : committed on Repositories and the harness × model table, and the first stage of the AI code journey funnel (generated → committed → merged → durable) on Impact.

**Proxy today:** the spend side exists. Requests after a session's last commit are "uncommitted work". The line side does not.

**Avenues:**

- **git-ai data.** Legacy `prompts` records carry `total_additions`, `total_deletions`, `accepted_lines` and `overriden_lines`. The v3 `sessions` records git-ai 1.7.x writes in this repository do not. `git ai stats --json` may report AI additions and deletions per range; it was far too slow for server-side ingestion (ADR, Rejected alternatives), but one commit or one session at session end may be fine. Checkpoint working logs are git-ai internals with no stability promise.
- **Edit-tool counts in our hooks** (M). The OMP extension already sees each `edit`, `apply_patch` and `write` call. Claude Code's `PostToolUse` hook sees `Edit`, `MultiEdit` and `Write` inputs. Both can count added and removed lines per call, keep a per-session total, and send it at session end. No content leaves the machine. Misses edits made by shell commands.
- **Gateway derivation** (M). With body inspection on, the gateway sees edit tool calls in responses and could count their lines. It cannot tell whether the call was applied, rejected or failed.

### 7. Line survival and churn at 14, 30 and 90 days

**Missing:** how many attested AI and human lines of a commit still exist N days later.

**Unlocks:** the whole Durability tab (survival curves, churn), cost per durable line, the durability side of Impact's counterweights, and the y-axis of the Repositories readiness quadrant.

**Proxy today:** none. Reverts are too coarse to stand in.

**Avenues:** survival needs line ranges, which the gateway never stores. So the computation must run where the repository and its notes live, and send counts only.

- **Scheduled CI job** (L). A cron workflow runs a new `oceans attribution survival` command. It reads notes for commits that turned 14, 30 or 90 days old, maps their attested ranges to `HEAD` with `git blame -M -C`, and posts `{sha, age_days, ai_surviving, human_surviving}` with a service-account key. Needs the notes ref pushed to the remote and a full-history clone. Blame is slow on large repositories, so batch per run.
- **Developer machines** (M). The same command run by the session-start hook at most once a day. Coverage follows whoever has the repository checked out. Results from two machines for one commit need the same freshness rules as notes.
- **Server-side clone job** (L). The ADR's clone backfill follow-up could also compute survival. Needs SCM credentials and the [SSRF rules](../adr/2026-10-17-ssrf-safe-outbound-fetch.md).
- **Re-reading later notes** does not work. A later note covers lines that commit added, not lines it removed from earlier commits.

### 8. AI lines rewritten in review

**Missing:** how many AI lines present when a PR opened (or became ready) were changed before merge.

**Unlocks:** a review rework share on Delivery and a "rewritten in review" column on the ledger.

**Proxy today:** after gap 1 lands, commits authored after the first review, split into AI and human lines.

**Avenues:** run the survival comparison of gap 7 between the PR head at ready-for-review and the head at merge. The App's `synchronize` events carry those SHAs. Squash merges make this harder because the merged commit has no note. Do this after gaps 1 and 7.

### 9. Human steering per session

**Missing:** user turns per session, so we can tell a 3-prompt session from a 40-prompt one.

**Unlocks:** autonomy on Repositories, the steering toggle of the harness × model table, and personal steering on My work.

**Proxy today:** growth of `message_count` across a session's requests. It also counts tool results and assistant turns, so it overstates steering.

**Avenues:**

- **Gateway derivation** (S). No new ingestion. When body inspection is on, count a request as a new user turn when its last message is a user message with text content. Anthropic Messages sends tool results with role `user`, so tool-result-only messages must not count. Responses API `input` items need the same split between `message` and `function_call_output`. Store one integer per request next to `message_count`. Never store the text. Caveats: compaction resets the message array, subagents share the parent session, and requests without body inspection have no count. Report coverage next to the figure.
- **`UserPromptSubmit` hook** (S). Claude Code fires it for each prompt, so `oceans hook claude` can send a count at session end without reading text. Exact for Claude Code only. Useful as a check on the gateway count.

### 10. Notes from other machines and CI

**Missing:** notes that never ran `oceans hook notes`: notes fetched from a remote, commits from machines without the hook, history before install, and notes written by Git AI's CI action for squash and rebase merges.

**Unlocks:** honest coverage meters on Impact and Repositories, and team totals that do not depend on everyone installing the hook.

**Proxy today:** the share of merged PR commits that have a stored note, per repository. Show it next to every team total.

**Avenues:**

- **Local backfill from `refs/notes/ai`** is cheap (`git cat-file --batch` read 1,037 notes in 0.03 s) but unsafe with a user key. Since the 2026-11-15 amendment, a note sent with a user's key credits that user, so a teammate's fetched note would be credited to the wrong person.
- **Service-account backfill** (M). Run the same read in CI, or a scheduled job, with a service-account key. Author emails then decide authorship, as they already do for service accounts. Session hashes are bound to the service account's scope, so those commits join no user's requests or cost. They count as lines without cost.
- **Git AI's CI action** plus `oceans hook notes` in the same job covers squash and rebase merges (ADR, Trade-offs).

### 11. Survey and satisfaction (optional)

**Missing:** how people feel about agent work: satisfaction, perceived speed, trust in AI code.

**Unlocks:** a SPACE and DX context strip on Impact and My work, so line counts are not the only signal.

**Avenues:** a short in-app pulse in the admin UI, or an import from an existing survey tool. Report only aggregates with a minimum group size (for example 5) and never per person. This is product work more than data plumbing. Leave it until the other gaps are done.

## Avenues

### Extending the `oceans` CLI hooks

Covers gaps 1, 2, 5, 6 and 9, partly 7 and 10. Uses the existing queue, owner binding and freshness rules, so new fields are mostly payload and schema changes. The data stays on the developer's machine until it is reduced to counts and hashes. Trade-offs: coverage depends on every developer installing the hooks and `gh`; data is only as fresh as the next session or hourly run; each repository is fetched by every developer who works in it, and records are duplicated per ownership scope.

### GitHub App

Covers gaps 1, 3 and 8 directly, and 2 and 10 in part.

- **Events:** `pull_request`, `pull_request_review`, `pull_request_review_comment`, `issue_comment`, `deployment`, `deployment_status`, `check_run`, `workflow_run` and `release`. Webhooks are signed with `X-Hub-Signature-256`.
- **Auth:** the App signs a JWT with its private key and exchanges it for an installation token that lasts an hour. The token is scoped to the repositories the org installed it on. Read-only permissions on pull requests, deployments, checks and contents metadata are enough.
- **Replaces in `oceans hook prs`:** the `gh` dependency, per-developer GitHub auth, the hourly delay, the merged-only limit, and duplicate PR rows per developer. One org-level install covers everyone, including people who never install the hooks.
- **Trade-offs:**
  - The gateway needs a public URL GitHub can reach. Self-hosted gateways behind a firewall would need a polling mode that uses the installation token instead. Polling needs the outbound fetch rules.
  - The ADR says the gateway needs no GitHub token. An App changes that, so it needs its own ADR: key storage, rotation, and which admins may install it.
  - Webhook PR payloads have no commit list. Each PR needs an extra API call for commits and their author emails.
  - Identities arrive as GitHub logins and user ids, not emails. Matching them to Oceans users needs a new per-user login list (like `additional_attribution_emails`) or a hash under the App's scope.
  - App data would belong to a service-account scope. The report's rule that a PR shows only notes its viewer can see must be checked for PRs owned by that scope.
  - Webhooks are not backfill. Installing the App still needs a one-time history import through the API.

### GitHub Actions or another CI step

Covers gaps 3, 7 and 10, and 1 or 2 where an App is not allowed. A job with a service-account key runs `oceans` and posts to the ingestion endpoint. It has the clone, the notes ref and the commit range, so it can do work the gateway cannot, such as blame and `rev-list`. Trade-offs: each repository must add the workflow; the key is a CI secret; minutes cost money on large repositories; and scheduled workflows on GitHub can be delayed or turned off on inactive repositories.

### GitLab and Bitbucket

GitLab group webhooks cover merge request, deployment, pipeline and release events, and group access tokens play the role of the App's installation token. `glab mr list` could stand in for `gh` in the hook. GitLab Ultimate also exposes DORA metrics through its API, which could feed Delivery directly. Bitbucket Cloud workspace webhooks cover pull request created, approved, merged and declined events. Deploy data would come from Pipelines through polling or a CI step. Both need their own identity matching and URL normalisation. Neither is worth building before a user asks.

### Monitoring and incident tools

Covers gap 4, and partly 3 through Sentry releases.

- **Tools:** PagerDuty and Opsgenie (incident triggered, acknowledged and resolved), incident.io (incident updates, with custom fields that can name a PR), Sentry (issue alerts and releases with suspect commits), Datadog monitor webhooks (templated payloads), Grafana alerting webhook contact points, Prometheus Alertmanager `webhook_config`, and status page incidents.
- **Gateway side:** one inbound route per provider, each with its own signature check (HMAC secret, bearer token or basic auth), replay protection and a body limit. Store service, severity, start and resolve times and an optional change reference. Drop titles and descriptions, which often carry customer data.
- **Trade-offs:** alert noise inflates failure rates unless only incidents (not every alert) count; every tool names services differently, so the service-to-repository map is admin work; and linking by time window blames whatever shipped last.

### CD systems

Argo CD notifications, Flux's notification controller and the GitHub Deployments API cover gap 3. Argo CD and Flux send generic webhooks on sync success or failure with the revision. Trade-offs: the revision is often a config repository's SHA, not the application's, so the deploy must be traced back to the application commit, usually through an image tag or a commit label; and multi-cluster setups send one event per cluster, so events need deduplication per environment.

### Server-side scheduled job

Covers gaps 7 and 10, and the polling form of the App. A gateway worker clones allowlisted repositories, reads notes and runs blame, then stores counts. It needs no developer install and no CI changes. Trade-offs: SCM credentials on the gateway, the [SSRF rules](../adr/2026-10-17-ssrf-safe-outbound-fetch.md), disk and CPU for clones, and it only sees notes that someone pushed to the remote.

### Gateway-internal derivation

Covers gap 9, the file-count part of gap 5 and the gateway part of gap 6. The data is already in the gateway at parse or request time. Trade-offs: counts depend on body inspection being on ([passive session analysis ADR](../adr/2026-07-21-passive-agent-session-analysis.md)), so coverage varies by deployment; request-level counting has to understand each API's message shapes; and new derived columns need a parser version so old rows are not misread.

## Recommended sequence

Smallest first. Each step should ship with its widget's pill changed from "Needs new data" to "Proxy" or "Live".

1. **PR lifecycle through `gh`.** Add `--state all` and the lifecycle, review, comment and size fields to `oceans hook prs`. Make `merged_at` optional and add `state`, `opened_at`, `closed_at`, `first_review_at` and the review and comment counts. Unlocks review wait, merged without review, abandon rate, state badges and the Sankey's review stages.
2. **User turns from gateway logs.** Add a user-turn count next to `message_count` in passive analysis. Optionally add the `UserPromptSubmit` count from `oceans hook claude`. Unlocks steering and autonomy.
3. **Revert matching.** Parse revert SHAs and PR numbers in `oceans hook prs`. Unlocks the change fail rate proxy and the reverts counterweight.
4. **File counts and numstat.** Count files per note on the gateway, and send numstat totals from the session-end hook. Unlocks true size and net change.
5. **GitHub App.** Exact lifecycle times, coverage beyond hook users and the deploy events of teams that use GitHub Deployments. Write an ADR first.
6. **Deploy and incident webhooks.** A CI step and CD webhooks for deploys, then one incident tool (whichever the first team uses) with a service-to-repository map. Turns the DORA proxies into real metrics.
7. **Generated lines.** Edit-tool counts from the Claude and OMP hooks at session end.
8. **Line survival.** The scheduled CI job, then review rewrites (gap 8) on top of it. Turns on Durability and the readiness quadrant.
9. **Coverage backfill and surveys** when teams ask for them.

## Open questions

- Should the GitHub App replace `oceans hook prs`, or should both run with one marked authoritative per repository? Running both duplicates PRs across ownership scopes.
- Which ownership scope owns App, CI and webhook data, and which viewers may see it? Today every record belongs to the key that sent it.
- How do we match GitHub logins to Oceans users without storing logins? A configured list, like `additional_attribution_emails`, or hashes only?
- Is time-window linking of incidents to deploys good enough to show, or should only explicitly linked incidents count?
- Should user-turn counting run only when body inspection is on, or is a header-free, body-free signal (the hook count) enough?
- Is survival measured against the default branch `HEAD` only, or per release branch too?
- Do we want directory buckets at all, given that hashed directory names are easy to guess?
- Should a "session failure rate" count reverts, incidents or both, and at what age does a session stop being blamed?
