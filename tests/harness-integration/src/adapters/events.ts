import { z } from "zod";

import type { ToolCall } from "../types.js";

const PiToolEventSchema = z.object({
  type: z.literal("tool_execution_start"),
  toolName: z.string(),
  args: z.unknown(),
});

const OpenCodeToolEventSchema = z.object({
  type: z.literal("tool_use"),
  part: z.object({
    tool: z.string(),
    state: z.object({ input: z.unknown() }).passthrough(),
  }),
});

const PiAgentEndSchema = z.object({
  type: z.literal("agent_end"),
  messages: z.array(z.unknown()),
});

const PiAssistantMessageSchema = z.object({
  role: z.literal("assistant"),
  content: z.unknown(),
  stopReason: z.string().optional(),
  errorMessage: z.string().optional(),
});

const TextContentSchema = z.object({
  type: z.literal("text"),
  text: z.string(),
});

const OpenCodeTextEventSchema = z.object({
  type: z.literal("text"),
  part: z.object({ text: z.string() }),
});

const OpenCodeErrorEventSchema = z.object({
  type: z.literal("error"),
  error: z.unknown(),
});

export function parseToolCalls(output: string): ToolCall[] {
  const toolCalls: ToolCall[] = [];
  for (const event of parseJsonEvents(output)) {
    const piEvent = PiToolEventSchema.safeParse(event);
    if (piEvent.success) {
      toolCalls.push({ input: piEvent.data.args, name: piEvent.data.toolName });
      continue;
    }
    const openCodeEvent = OpenCodeToolEventSchema.safeParse(event);
    if (openCodeEvent.success) {
      toolCalls.push({
        input: openCodeEvent.data.part.state.input,
        name: openCodeEvent.data.part.tool,
      });
    }
  }
  return toolCalls;
}

export function parseAssistantOutput(output: string): string {
  let assistantOutput: string | undefined;
  let terminalError: unknown;
  for (const event of parseJsonEvents(output)) {
    const piAgentEnd = PiAgentEndSchema.safeParse(event);
    if (piAgentEnd.success) {
      for (const candidate of piAgentEnd.data.messages) {
        const message = PiAssistantMessageSchema.safeParse(candidate);
        if (!message.success) {
          continue;
        }
        if (message.data.errorMessage || ["aborted", "error"].includes(message.data.stopReason ?? "")) {
          terminalError =
            message.data.errorMessage ?? `Pi stopped with reason ${message.data.stopReason}`;
          continue;
        }
        const text = messageText(message.data.content);
        if (text) {
          assistantOutput = text;
        }
      }
      continue;
    }

    const openCodeText = OpenCodeTextEventSchema.safeParse(event);
    if (openCodeText.success && openCodeText.data.part.text.trim()) {
      assistantOutput = openCodeText.data.part.text.trim();
      continue;
    }
    const openCodeError = OpenCodeErrorEventSchema.safeParse(event);
    if (openCodeError.success) {
      terminalError = openCodeError.data.error;
    }
  }

  if (terminalError !== undefined) {
    throw new Error(`Harness reported an agent error: ${JSON.stringify(terminalError)}`);
  }
  if (!assistantOutput) {
    throw new Error("Harness JSON stream did not contain a final assistant response");
  }
  return assistantOutput;
}

function messageText(content: unknown): string {
  if (typeof content === "string") {
    return content.trim();
  }
  if (!Array.isArray(content)) {
    return "";
  }
  return content
    .flatMap((block) => {
      const text = TextContentSchema.safeParse(block);
      return text.success ? [text.data.text] : [];
    })
    .join("")
    .trim();
}

function parseJsonEvents(output: string): unknown[] {
  const events: unknown[] = [];
  for (const line of output.split("\n")) {
    if (!line.startsWith("{")) {
      continue;
    }
    try {
      events.push(JSON.parse(line));
    } catch {
      // Ignore non-event diagnostics interleaved with the JSON stream.
    }
  }
  return events;
}
