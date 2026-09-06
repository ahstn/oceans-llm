# Reviewfrog OpenCode agent definition

Source: [agents/opencodeShared.ts:190](https://github.com/pullfrog/pullfrog/blob/0212dedb0f92b8ba4020c17dc30d3eced32415d7/agents/opencodeShared.ts#L190) · Commit: `0212dedb0f92b8ba4020c17dc30d3eced32415d7`.

Exact registration function. REVIEWER_SYSTEM_PROMPT refers to reviewfrog-system-prompt.md. The model override comes from deriveSubagentModels.

```typescript
export function buildReviewerAgentConfig(
  orchestratorModel: string | undefined
): Record<string, unknown> {
  const overrides = deriveSubagentModels(orchestratorModel);
  return {
    [REVIEWER_AGENT_NAME]: {
      description:
        "Read-only review subagent for lens-based code review (correctness, security, billing-subsystem, etc.). " +
        "Reads only — no writes, no state-changing shell or MCP calls, no nested subagent dispatch.",
      mode: "subagent",
      prompt: REVIEWER_SYSTEM_PROMPT,
      ...(overrides.reviewer !== undefined ? { model: overrides.reviewer } : {}),
    },
  };
}

```
