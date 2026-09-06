# Pi review agent session definition

Source: [packages/warden/src/sdk/runtimes/pi.ts:575](https://github.com/getsentry/warden/blob/6d361c0473a3236cc31c4fbe4a0a281b84679eb8/packages/warden/src/sdk/runtimes/pi.ts#L575) · Commit: `6d361c0473a3236cc31c4fbe4a0a281b84679eb8`.

Exact resource-loader configuration. Warden supplies the system prompt and disables automatic skill, extension, template, theme, and context-file loading.

The session creation excerpt follows below.

```typescript
const agentDir = getAgentDir();
  const resourceLoader = new DefaultResourceLoader({
    cwd: options.cwd,
    agentDir,
    settingsManager,
    noExtensions: true,
    noSkills: true,
    noPromptTemplates: true,
    noThemes: true,
    noContextFiles: true,
    systemPrompt: options.systemPrompt,
  });
  await resourceLoader.reload();
```

Session creation ([source](https://github.com/getsentry/warden/blob/6d361c0473a3236cc31c4fbe4a0a281b84679eb8/packages/warden/src/sdk/runtimes/pi.ts#L730)):

```typescript
    const result = await createAgentSession({
      cwd: options.cwd,
      agentDir,
      modelRuntime,
      model,
      thinkingLevel: options.effort,
      tools: options.toolNames,
      noTools: options.toolNames.length === 0 ? 'all' : undefined,
      customTools: sessionCustomTools.length > 0 ? sessionCustomTools : undefined,
      resourceLoader,
      sessionManager: SessionManager.inMemory(options.cwd),
      settingsManager,
    });
```
