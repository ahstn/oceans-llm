# GitHub Review Publication

GitHub PR base/head SHAs and the matching diff define the review. In the action, use the immutable context and diff supplied by the host. Do not re-derive scope from the worker's isolated current directory. When using a separate interactive host with GitHub access, retrieve metadata, the full paginated changed-file list, diff, reviews, and review threads before posting. Use `gh` there; review threads require the GraphQL API, not an unsupported `gh pr view` JSON field.

A local checkout is authoritative only when it matches the reviewed head. Check the current head before publishing when the host exposes that operation; if it moved, refresh the diff and findings. The action worker cannot query the current GitHub head and must use the supplied SHA without claiming a live refresh.

## Existing comments

Deduplicate by root cause and remedy, including findings whose code moved. An outdated anchor does not prove a fix. Do not resolve, delete, or rewrite another reviewer's thread as part of this review. Thread triage requires a separate authorized task and evidence that every substantive concern is addressed.

This worker does not receive existing review threads or linked issue contents. Record those limits in `degraded_features` when the review would require them. Do not claim semantic deduplication against unseen comments, and do not infer an issue's requirements from its number or title.

## Submission

Draft and verify findings before publication. In this action, pass them to `submit_review` for host publication. Use repository-relative paths and changed RIGHT-side line numbers. Keep one finding per issue, anchored to the smallest line that demonstrates the change. For a deletion-only or cross-file issue with no valid RIGHT-side anchor, explain the concrete concern in the summary; never attach it to an unrelated line.

Put the primary axis and evidence in `message`; put severity in `severity`. Use only `low`, `medium`, `high`, or `critical`. The host adds the severity prefix, applies comment limits, and determines review events. Do not submit approval verdicts or duplicate top-level “LGTM” comments.

A successful tool response confirms acceptance for publication, not that GitHub has already stored the comments. Stop after acceptance and let the publisher report success or failure. If an interactive host publishes directly, use a structured API payload or a body file, verify the returned comment IDs, and do not blindly retry a write with an uncertain result.
