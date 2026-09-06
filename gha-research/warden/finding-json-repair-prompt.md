# Finding JSON extraction repair prompt

Source: [packages/warden/src/sdk/extract.ts:225](https://github.com/getsentry/warden/blob/6d361c0473a3236cc31c4fbe4a0a281b84679eb8/packages/warden/src/sdk/extract.ts#L225) · Commit: `6d361c0473a3236cc31c4fbe4a0a281b84679eb8`.

Rendered from the source prompt builder with labelled runtime placeholders. No live repository data or model request was used. Optional historical evidence and skill-resource paths are described in runtime-flow.md.

<task>
Extract the findings JSON from this model output.
</task>

<output_format>
Return only valid JSON. Do not include markdown, prose, code fences, or explanations.

Return this shape: {"findings": [...]}
If no findings exist, return: {"findings": []}
</output_format>

<model_output>
{{truncated_model_output}}
</model_output>
