import { z } from "zod";

const PiToolEventSchema = z.object({
  type: z.literal("tool_execution_start"),
  toolName: z.string(),
});

const OpenCodeToolEventSchema = z.object({
  type: z.literal("tool_use"),
  part: z.object({ tool: z.string() }),
});

export function parseToolCalls(output: string): string[] {
  const toolCalls: string[] = [];
  for (const line of output.split("\n")) {
    if (!line.startsWith("{")) {
      continue;
    }
    let event: unknown;
    try {
      event = JSON.parse(line);
    } catch {
      continue;
    }
    const piEvent = PiToolEventSchema.safeParse(event);
    if (piEvent.success) {
      toolCalls.push(piEvent.data.toolName);
      continue;
    }
    const openCodeEvent = OpenCodeToolEventSchema.safeParse(event);
    if (openCodeEvent.success) {
      toolCalls.push(openCodeEvent.data.part.tool);
    }
  }
  return toolCalls;
}
