# Reviewfrog Claude Code agent definition

Source: [agents/claude.ts:184](https://github.com/pullfrog/pullfrog/blob/0212dedb0f92b8ba4020c17dc30d3eced32415d7/agents/claude.ts#L184) · Commit: `0212dedb0f92b8ba4020c17dc30d3eced32415d7`.

Exact registration function. REVIEWER_SYSTEM_PROMPT refers to reviewfrog-system-prompt.md. This definition selects claude-sonnet-5; task-level model overrides may take precedence.

```typescript
function buildAgentsJson(): string {
  const agents = {
    [REVIEWER_AGENT_NAME]: {
      description:
        "Read-only review subagent for lens-based code review (correctness, security, billing-subsystem, etc.). " +
        "Reads only — no writes, no state-changing shell or MCP calls, no nested subagent dispatch.",
      prompt: REVIEWER_SYSTEM_PROMPT,
      model: "claude-sonnet-5",
    },
  };
  return JSON.stringify(agents);
}

```
