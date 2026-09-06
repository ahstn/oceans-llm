# Batch finding consolidation prompt

Source: [packages/warden/src/output/dedup.ts:821](https://github.com/getsentry/warden/blob/6d361c0473a3236cc31c4fbe4a0a281b84679eb8/packages/warden/src/output/dedup.ts#L821) · Commit: `6d361c0473a3236cc31c4fbe4a0a281b84679eb8`.

Rendered from the source prompt builder with labelled runtime placeholders. No live repository data or model request was used. Optional historical evidence and skill-resource paths are described in runtime-flow.md.

<task>
Group findings that describe the SAME root cause or bug.
</task>

<findings>
{{indexed_new_findings}}
</findings>

<deduplication_rules>
Return a JSON array of arrays, where each inner array contains the 1-based indices of findings that describe the same root cause.
Only group findings that are truly about the same underlying issue. Findings about different issues should NOT be grouped even if they're nearby.
Singletons (findings with no duplicates) should not appear in any group.
</deduplication_rules>

<output_format>
Return only valid JSON. Do not include markdown, prose, code fences, or explanations.

Return the JSON array. Return [] if no findings share a root cause.
</output_format>
