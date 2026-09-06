# Review body and technical-details format

Source: [modes.ts:20](https://github.com/pullfrog/pullfrog/blob/0212dedb0f92b8ba4020c17dc30d3eced32415d7/modes.ts#L20) · Commit: `0212dedb0f92b8ba4020c17dc30d3eced32415d7`.

Shared prompt fragment embedded in both review modes.

### Default format

The body has at most three parts, in this order:

1. **Reviewed changes preamble** — a bolded `**Reviewed changes**` lead-in with one sentence on what was reviewed in this run (for `IncrementalReview`: what changed since the prior pullfrog review), then a bullet list of the substantive changes — short bolded title, one sentence each. A reviewer should understand the full reviewed scope from this list alone. Close the preamble with the metadata comment below.
2. **Cross-cutting issue sections** (zero or more) — one `### {emoji} {what's wrong, not what to do}` heading per concern.
3. **`### ℹ️ Nitpicks`** at the very bottom, if any — a flat bullet list, no technical-details block.

**Inline vs. body.** Concerns that anchor to a specific line go inline (the `comments` parameter), even when their implications are broad. Body `### ` sections are reserved for concerns that have **no line to anchor to** — *absence* (something the diff should have done but didn't), *sequencing* (rollout / deletion / migration order), *design decisions only the human can make*, or *scope questions the diff raises but doesn't address*. With no non-anchorable concerns, the body is just the preamble + metadata.

**Severity emoji** on every `### ` heading, and nowhere else: 🚨 critical (blocks merge — data loss, security, broken core flow) · ⚠️ important (must address before merging) · ℹ️ informational (mergeable as-is).

**Blank line between every block-level element.** GitHub's markdown parser requires one before and after HTML tags (`<details>`, `<summary>`, `<sub>`, `<br/>`) — without it GitHub treats what follows as a continuation of the HTML block and renders your markdown as literal text. This is a parser quirk, not a style preference, and it permanently breaks the posted review.

## Metadata comment

Fill every field from the `checkout_pr` response — never count files or commits by hand. For `IncrementalReview`, fill `Prior pullfrog review` from `list_pull_request_reviews`.

```
<!--
Pullfrog review metadata. These findings were written against {head_sha_short};
if commits have landed on {head_ref} since, treat every specific bug, file, or
line callout as POTENTIALLY STALE and re-diff before acting on it.

- Mode: Review (initial)   or   IncrementalReview (delta against prior pullfrog review)
- Files reviewed: {file_count}
- Commits reviewed: {commit_count}
- Base: {base_ref} ({base_sha_short})
- Head: {head_ref} ({head_sha_short})
- Reviewed commits:
  - {sha_short} — {commit_subject}
- Prior pullfrog review: none   or   {prior_sha_short} ({prior_review_html_url})
-->
```

## Technical details

Every body `### ` section carries one; an inline comment carries one when its fix is non-trivial or spans files. The visible part above it states the PROBLEM in 2-3 sentences — what's broken and what the blast radius is. Asks, fixes, and open questions live inside the block, which a downstream fix-agent pulls down as its brief, so `file:line` refs and identifier density belong here.

```
<details><summary>Technical details</summary>

\`\`\`\`markdown
# {title}

## Affected sites
- {file path:line} — {what's wrong there}

## Required outcome
- {what the fix needs to achieve, not how to achieve it}

## Suggested approach (optional)
## Open questions for the human (optional)
\`\`\`\`

</details>
```

The 4-backtick fence lets the block hold its own 3-backtick fences and stay one-click copyable. Skip the optional sections when they'd add nothing.

Backtick-wrap identifiers and file names. Don't repeat diff content, don't include raw `+123 / -45` stats, no changelog, no horizontal rules, and no `### Key changes` / `### Issues found` / `<b>TL;DR</b>` heading — each `### ` heading IS the issue.
