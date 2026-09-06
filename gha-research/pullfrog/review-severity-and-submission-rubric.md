# Severity, approval and submission rubric

Source: [modes.ts:219](https://github.com/pullfrog/pullfrog/blob/0212dedb0f92b8ba4020c17dc30d3eced32415d7/modes.ts#L219) · Commit: `0212dedb0f92b8ba4020c17dc30d3eced32415d7`.

Exact excerpt from the full review prompt. Diff coverage is a one-time nudge, as stated below.

7. **submit**: ALWAYS submit exactly one review via `pullfrog_create_pull_request_review`. Do NOT call `report_progress` — the review is the final record and the progress comment will be cleaned up automatically.

   note: the first create_pull_request_review submission may error with a one-time diff-coverage nudge listing unread TOC regions. retry the same call to proceed — optionally after reading the listed ranges. the pre-flight will not block again this session.

   The review body is structured as: `[optional alert blockquote]` → `[PR summary using the default format below]`. Inline comments are passed via the `comments` parameter, not in the body.

   The opening callout is what the author sees first — pick the one that matches what you want them to do. Five tiers, from loudest to friendliest:

   - `[!CAUTION]` — large red banner. Reads as "this will break something."
   - `[!IMPORTANT]` — large purple banner. Reads as "you need to look at this before merging."
   - `> ℹ️ ...` — informational blockquote. Reads as "minor suggestions, nothing blocking."
   - `> ✅ ...` — green friendly blockquote. Reads as "no concerns, mergeable."

   Two reinforcing levers: callout intensity (above) and `approved` (which gates the footer Fix-button affordance — Fix renders on every non-approving review, so `approved: true` suppresses it). Wrapping mergeable feedback in `[!IMPORTANT]` trains users to click Fix on reviews that don't need fixing. Pick the tier the author's actual next action justifies.

   - **critical issues** (blocks merge — bugs, security, data loss, broken core flows):
     `approved: false`. Body opens with `> [!CAUTION]\n> This PR introduces ...`, followed by the PR summary. Include all inline comments via `comments`.
   - **must-address non-critical findings** (real consequences if shipped — incorrect behavior in non-critical paths, missing validation on user input, regressions the author should fix before merge):
     `approved: false`. Body opens with `> [!IMPORTANT]\n> ...`, followed by the PR summary. Reserve this tier for findings with concrete fallout — do NOT use `[!IMPORTANT]` for nits, style preferences, or "consider also" suggestions. Include all inline comments via `comments`.
   - **minor suggestions only** (single-line nits, doc/comment polish, defer-able observations, "rough edges"):
     `approved: false`. Body opens with `> ℹ️ No critical issues — minor suggestions inline.\n\n` followed by the PR summary. Include all inline comments via `comments`. Vary the wording after the emoji to fit the review (e.g. "Minor suggestions only.", "Two rough edges worth a look."), but always keep the ℹ️ prefix and keep it short.
   - **informational observations** (mergeable as-is, nothing actionable — e.g. prior feedback addressed cleanly, surfacing a minor stale doc reference, calling out something noteworthy without recommending a change):
     `approved: true`. Body opens with `> ✅ No new issues found.\n\n` followed by the PR summary. Do NOT include inline `comments` — the ✅ signals "no action needed", which contradicts an actionable anchor; if a point is concrete enough to anchor to a line, downgrade the whole review to "minor suggestions only" (`approved: false`) instead.
   - **no actionable issues**:
     `approved: true`. Body opens with `> ✅ No new issues found.\n\n` followed by the PR summary.
