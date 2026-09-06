# Hunk analysis user prompt

Source: [packages/warden/src/sdk/prompt.ts:103](https://github.com/getsentry/warden/blob/6d361c0473a3236cc31c4fbe4a0a281b84679eb8/packages/warden/src/sdk/prompt.ts#L103) · Commit: `6d361c0473a3236cc31c4fbe4a0a281b84679eb8`.

Rendered from the source prompt builder with labelled runtime placeholders. No live repository data or model request was used. Optional historical evidence and skill-resource paths are described in runtime-flow.md.

<task>
Analyze this code change according to the "{{skill_name}}" skill criteria.
</task>

<pull_request_context>
<repository>{{owner/repo}}</repository>
<title>{{pr_title}}</title>
<body>
{{pr_body}}
</body>
</pull_request_context>

<changed_files>
- {{changed_file_path}}
</changed_files>

{{formatted_hunk_with_line_range_and_context}}

<scope_reminder>
Only report findings that are explicitly covered by the skill instructions. Do not report general code quality issues, bugs, or improvements unless the skill specifically asks for them. Return an empty findings array if no issues match the skill's criteria.
</scope_reminder>
