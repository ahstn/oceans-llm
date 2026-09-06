# Existing-comment semantic deduplication prompt

Source: [packages/warden/src/output/dedup.ts:512](https://github.com/getsentry/warden/blob/6d361c0473a3236cc31c4fbe4a0a281b84679eb8/packages/warden/src/output/dedup.ts#L512) · Commit: `6d361c0473a3236cc31c4fbe4a0a281b84679eb8`.

Rendered from the source prompt builder with labelled runtime placeholders. No live repository data or model request was used. Optional historical evidence and skill-resource paths are described in runtime-flow.md.

<task>
Compare these code review findings and identify duplicates.
</task>

<existing_comments>
{{indexed_existing_review_comments}}
</existing_comments>

<new_findings>
{{indexed_new_findings}}
</new_findings>

<deduplication_rules>
Return a JSON array of objects identifying which findings are DUPLICATES of which existing comments.
Only mark as duplicate if they describe the SAME issue at the SAME location (within a few lines).
Different issues at the same location are NOT duplicates.
</deduplication_rules>

<output_format>
Return only valid JSON. Do not include markdown, prose, code fences, or explanations.

[{"findingIndex": 1, "existingIndex": 2}]
where findingIndex is the 1-based index of the new finding and existingIndex is the 1-based index of the matching existing comment.
Return [] if none are duplicates.
</output_format>
