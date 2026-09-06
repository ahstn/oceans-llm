# Cross-location root-cause merge prompt

Source: [packages/warden/src/sdk/extract.ts:557](https://github.com/getsentry/warden/blob/6d361c0473a3236cc31c4fbe4a0a281b84679eb8/packages/warden/src/sdk/extract.ts#L557) · Commit: `6d361c0473a3236cc31c4fbe4a0a281b84679eb8`.

Rendered from the source prompt builder with labelled runtime placeholders. No live repository data or model request was used. Optional historical evidence and skill-resource paths are described in runtime-flow.md.

<task>
Identify which of these code review findings describe the SAME underlying issue appearing at different locations. Group them by shared root cause.
</task>

<findings>
{{indexed_findings_with_severity_confidence_and_code_snippets}}
</findings>

<output_format>
Return only valid JSON. Do not include markdown, prose, code fences, or explanations.

Return a JSON array of arrays, where each inner array contains the 1-based indices of findings about the same issue.
Singletons should not appear. Return [] if no findings describe the same issue.
</output_format>
