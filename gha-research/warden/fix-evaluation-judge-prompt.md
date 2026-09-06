# Fix-evaluation judge prompt

Source: [packages/warden/src/action/fix-evaluation/judge.ts:71](https://github.com/getsentry/warden/blob/6d361c0473a3236cc31c4fbe4a0a281b84679eb8/packages/warden/src/action/fix-evaluation/judge.ts#L71) · Commit: `6d361c0473a3236cc31c4fbe4a0a281b84679eb8`.

Rendered from the source prompt builder with labelled runtime placeholders. No live repository data or model request was used. Optional historical evidence and skill-resource paths are described in runtime-flow.md. This render selects the branch with after-code available.

<task>
Judge whether a code change fixed a reported issue.
</task>

<key_question>
Does the reported issue still exist in the code after this commit?
</key_question>

<verdict_definitions>
Choose ONE verdict based on these criteria:

resolved - The issue NO LONGER EXISTS. Evidence:
- The problematic code was corrected (directly or via equivalent fix)
- The code was refactored in a way that eliminates the issue by design
- The problematic code was intentionally removed (file deleted, function removed, dead code cleaned up)

attempted_failed - A fix was CLEARLY ATTEMPTED but the issue PERSISTS. Evidence:
- Changes DIRECTLY modify the reported file at or near the issue location
- AND the changes appear specifically intended to address THIS issue
- BUT the core issue remains (wrong fix, incomplete fix, edge cases missed)
- Use this ONLY when there's clear evidence of intent to fix THIS specific issue
- Do NOT use for general refactoring, unrelated bug fixes, or changes to other files
- When in doubt between attempted_failed and not_attempted, prefer not_attempted

not_attempted - The issue was NOT ADDRESSED. Evidence:
- No changes to the problematic code or its logic
- Changes are unrelated (different feature, different bug, unrelated refactor)
- The reported code is identical or functionally unchanged
- Changes are in other files with no clear connection to the reported issue
</verdict_definitions>

<reported_issue>
<title>{{reported_title}}</title>
<file>{{file_path}}</file>
<line>{{reported_line}}</line>
<description>
{{reported_description}}
</description>
</reported_issue>

<before_code>
{{before_code}}
</before_code>

<after_code>
{{after_code}}
</after_code>

<changed_files>
- {{changed_file_path}}
</changed_files>

<developer_intent>
1. {{commit_subject}}

Use these to help understand what the developer was trying to do. A commit mentioning "fix" or the issue topic suggests intent to address it.
</developer_intent>

<investigation_strategy>
Compare the BEFORE and AFTER code above to determine if the issue was fixed.
Use tools only if you need additional context:

- `get_file_diff(path)` - See unified diff of changes to a file
- `get_file_at_commit(path, "before"|"after", startLine?, endLine?)` - Read more file content if needed
</investigation_strategy>

<output_format>
Return only valid JSON. Do not include markdown, prose, code fences, or explanations.

{"status": "resolved|attempted_failed|not_attempted", "reasoning": "One sentence explaining your verdict"}
Put your one-sentence explanation in the "reasoning" field.
</output_format>
