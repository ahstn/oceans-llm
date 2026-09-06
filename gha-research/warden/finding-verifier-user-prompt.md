# Finding verifier user prompt

Source: [packages/warden/src/sdk/verify.ts:107](https://github.com/getsentry/warden/blob/6d361c0473a3236cc31c4fbe4a0a281b84679eb8/packages/warden/src/sdk/verify.ts#L107) · Commit: `6d361c0473a3236cc31c4fbe4a0a281b84679eb8`.

Rendered from the source prompt builder with labelled runtime placeholders. No live repository data or model request was used. Optional historical evidence and skill-resource paths are described in runtime-flow.md.

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

<candidate_finding>
{
  "id": "{{finding_id}}",
  "title": "{{title}}",
  "description": "{{description}}",
  "severity": "{{severity}}",
  "confidence": "{{confidence}}",
  "verification": "{{evidence_trace}}",
  "location": {
    "path": "{{file_path}}",
    "startLine": "{{line}}"
  }
}
</candidate_finding>

<task>
Verify this candidate. Return keep, revise, or reject.
</task>
