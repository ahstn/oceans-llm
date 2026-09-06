# Review mode instruction composition

Source: [mcp/selectMode.ts:44](https://github.com/pullfrog/pullfrog/blob/0212dedb0f92b8ba4020c17dc30d3eced32415d7/mcp/selectMode.ts#L44) · Commit: `0212dedb0f92b8ba4020c17dc30d3eced32415d7`.

Built-in prompt text is combined with configured mode instructions. Account and repository values are runtime data and are not available in this clone.

```typescript
// IncrementalReview inherits Review's user instructions, Fix inherits Build's
const modeInstructionParent: Record<string, string> = {
  IncrementalReview: "Review",
  Fix: "Build",
};

function buildOrchestratorGuidance(
  ctx: ToolContext,
  mode: Mode,
  overrideGuidance?: string
): OrchestratorGuidance {
  const hardcoded = overrideGuidance ?? mode.prompt ?? "";
  const lookupKey = modeInstructionParent[mode.name] ?? mode.name;
  const userInstructions = ctx.modeInstructions[lookupKey] ?? "";
  const guidance = [hardcoded, userInstructions].filter(Boolean).join("\n\n");
  return {
    modeName: mode.name,
    description: mode.description,
    orchestratorGuidance: guidance,
  };
}

```
