# Claude review agent query definition

Source: [packages/warden/src/sdk/runtimes/claude.ts:392](https://github.com/getsentry/warden/blob/6d361c0473a3236cc31c4fbe4a0a281b84679eb8/packages/warden/src/sdk/runtimes/claude.ts#L392) · Commit: `6d361c0473a3236cc31c4fbe4a0a281b84679eb8`.

Exact query setup, including its diagnostic callback. Read-only review tools are selected by resolveClaudeSkillTools; Task and TodoWrite are denied.

```typescript
const stream = query({
          prompt: userPrompt,
          options: {
            maxTurns,
            cwd: repoPath,
            systemPrompt,
            // Hunk analysis is read-only; trusted internal writer tasks may opt
            // into mutating tools explicitly at the runtime request boundary.
            allowedTools: skillTools.allowedTools,
            disallowedTools: skillTools.disallowedTools,
            permissionMode: 'bypassPermissions',
            // Prevent SDK from writing session .jsonl files and polluting Claude Code's session index.
            persistSession: false,
            env: claudeEnv(),
            model,
            ...effortOptions(effort),
            abortController,
            pathToClaudeCodeExecutable,
            stderr: (data: string) => {
              stderrChunks.push(data);
            },
          },
        });
```
