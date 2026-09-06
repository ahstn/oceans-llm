# Fix-evaluation judge prompt builder

Source: [packages/warden/src/action/fix-evaluation/judge.ts:71](https://github.com/getsentry/warden/blob/6d361c0473a3236cc31c4fbe4a0a281b84679eb8/packages/warden/src/action/fix-evaluation/judge.ts#L71) · Commit: `6d361c0473a3236cc31c4fbe4a0a281b84679eb8`.

Exact builder, preserving both investigation branches and optional context.

```typescript
function buildPrompt(input: FixJudgeInput): string {
  const { comment, changedFiles, codeBeforeFix, codeAfterFix, commitMessages } = input;

  const afterCodeSection = codeAfterFix
    ? buildTaggedSection('after_code', codeAfterFix)
    : undefined;

  const commitMessagesSection =
    commitMessages && commitMessages.length > 0
      ? buildTaggedSection('developer_intent', [
          ...commitMessages.map((msg, i) => `${i + 1}. ${msg.split('\n')[0]}`),
          '',
          'Use these to help understand what the developer was trying to do. A commit mentioning "fix" or the issue topic suggests intent to address it.',
        ])
      : undefined;

  const investigationStrategy = codeAfterFix
    ? `Compare the BEFORE and AFTER code above to determine if the issue was fixed.
Use tools only if you need additional context:

- \`get_file_diff(path)\` - See unified diff of changes to a file
- \`get_file_at_commit(path, "before"|"after", startLine?, endLine?)\` - Read more file content if needed`
    : `Use tools to determine if the issue was fixed:

1. **Start with get_file_diff** on the issue's file (if changed) to see what was modified
2. **Use get_file_at_commit with "after"** to see the current state at the issue location
3. **Check related files** if the fix might involve changes elsewhere (imports, shared utilities, etc.)

Tools:
- \`get_file_diff(path)\` - See unified diff of changes to a file
- \`get_file_at_commit(path, "before"|"after", startLine?, endLine?)\` - Read file content at either commit`;

  return joinPromptSections([
    `<task>
Judge whether a code change fixed a reported issue.
</task>`,
    `<key_question>
Does the reported issue still exist in the code after this commit?
</key_question>`,
    `<verdict_definitions>
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
</verdict_definitions>`,
    buildTaggedSection('reported_issue', [
      `<title>${comment.title}</title>`,
      `<file>${comment.path}</file>`,
      `<line>${comment.line}</line>`,
      '<description>',
      comment.description,
      '</description>',
    ]),
    buildTaggedSection('before_code', codeBeforeFix),
    afterCodeSection,
    buildFileListSection('changed_files', changedFiles),
    commitMessagesSection,
    buildTaggedSection('investigation_strategy', investigationStrategy),
    buildJsonOutputSection(`{"status": "resolved|attempted_failed|not_attempted", "reasoning": "One sentence explaining your verdict"}
Put your one-sentence explanation in the "reasoning" field.`),
  ]);
}
```
