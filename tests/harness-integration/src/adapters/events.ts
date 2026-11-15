import { z } from "zod";

import type { ToolCall } from "../types.js";

const PiToolEventSchema = z.object({
  type: z.literal("tool_execution_start"),
  toolCallId: z.string(),
  toolName: z.string(),
  args: z.unknown(),
});

const PiToolEndEventSchema = z.object({
  type: z.literal("tool_execution_end"),
  toolCallId: z.string(),
  // OMP omits `isError` on success.
  isError: z.boolean().optional(),
});

const OpenCodeToolEventSchema = z.object({
  type: z.literal("tool_use"),
  part: z.object({
    tool: z.string(),
    state: z
      .object({ input: z.unknown(), status: z.enum(["completed", "error"]) })
      .passthrough(),
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

// Claude Code `--output-format stream-json`: tool calls arrive as content blocks of assistant
// messages, and their results as content blocks of the next user message.
const ClaudeMessageEventSchema = z.object({
  type: z.enum(["assistant", "user"]),
  message: z.object({ content: z.unknown() }),
});

const ClaudeToolUseSchema = z.object({
  type: z.literal("tool_use"),
  id: z.string(),
  name: z.string(),
  input: z.unknown(),
});

const ClaudeToolResultSchema = z.object({
  type: z.literal("tool_result"),
  tool_use_id: z.string(),
  is_error: z.boolean().optional(),
});

const ClaudeResultEventSchema = z.object({
  type: z.literal("result"),
  subtype: z.string(),
  is_error: z.boolean(),
  result: z.string().optional(),
});

export function parseToolCalls(output: string): ToolCall[] {
  const toolCalls: ToolCall[] = [];
  const toolCallsById = new Map<string, ToolCall>();
  for (const event of parseJsonEvents(output)) {
    const piEvent = PiToolEventSchema.safeParse(event);
    if (piEvent.success) {
      const toolCall: ToolCall = {
        input: piEvent.data.args,
        name: piEvent.data.toolName,
        status: "started",
      };
      toolCalls.push(toolCall);
      toolCallsById.set(piEvent.data.toolCallId, toolCall);
      continue;
    }
    const piEndEvent = PiToolEndEventSchema.safeParse(event);
    if (piEndEvent.success) {
      const toolCall = toolCallsById.get(piEndEvent.data.toolCallId);
      if (toolCall) {
        toolCall.status = piEndEvent.data.isError ? "error" : "completed";
      }
      continue;
    }
    const openCodeEvent = OpenCodeToolEventSchema.safeParse(event);
    if (openCodeEvent.success) {
      toolCalls.push({
        input: openCodeEvent.data.part.state.input,
        name: openCodeEvent.data.part.tool,
        status: openCodeEvent.data.part.state.status,
      });
      continue;
    }
    const claudeMessage = ClaudeMessageEventSchema.safeParse(event);
    if (claudeMessage.success && Array.isArray(claudeMessage.data.message.content)) {
      for (const block of claudeMessage.data.message.content) {
        const toolUse = ClaudeToolUseSchema.safeParse(block);
        if (toolUse.success) {
          const toolCall: ToolCall = {
            input: toolUse.data.input,
            name: toolUse.data.name,
            status: "started",
          };
          toolCalls.push(toolCall);
          toolCallsById.set(toolUse.data.id, toolCall);
          continue;
        }
        const toolResult = ClaudeToolResultSchema.safeParse(block);
        if (!toolResult.success) {
          continue;
        }
        const toolCall = toolCallsById.get(toolResult.data.tool_use_id);
        if (toolCall) {
          toolCall.status = toolResult.data.is_error ? "error" : "completed";
        }
      }
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
      continue;
    }
    const claudeResult = ClaudeResultEventSchema.safeParse(event);
    if (claudeResult.success) {
      if (claudeResult.data.is_error || claudeResult.data.subtype !== "success") {
        terminalError = event;
      } else if (claudeResult.data.result?.trim()) {
        assistantOutput = claudeResult.data.result.trim();
      }
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
