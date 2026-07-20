import { describe, expect, test } from "vitest";

import { parseAssistantOutput, parseToolCalls } from "./events.js";

describe("harness event parsing", () => {
  test("returns only Pi's final assistant text", () => {
    const output = [
      JSON.stringify({
        type: "agent_end",
        messages: [
          { role: "user", content: [{ type: "text", text: "EXPECTED_MARKER" }] },
          {
            role: "assistant",
            content: [{ type: "text", text: "actual response" }],
            stopReason: "stop",
          },
        ],
      }),
    ].join("\n");

    expect(parseAssistantOutput(output)).toBe("actual response");
  });

  test("rejects Pi error termination", () => {
    const output = JSON.stringify({
      type: "agent_end",
      messages: [
        {
          role: "assistant",
          content: [],
          errorMessage: "provider failed",
          stopReason: "error",
        },
      ],
    });

    expect(() => parseAssistantOutput(output)).toThrow(/provider failed/);
  });

  test("returns OpenCode text and rejects terminal errors", () => {
    expect(
      parseAssistantOutput(
        JSON.stringify({ type: "text", part: { text: "OpenCode response" } }),
      ),
    ).toBe("OpenCode response");
    expect(() =>
      parseAssistantOutput(JSON.stringify({ type: "error", error: { name: "ProviderError" } })),
    ).toThrow(/ProviderError/);
  });

  test("preserves tool inputs from Pi and OpenCode events", () => {
    const output = [
      JSON.stringify({
        type: "tool_execution_start",
        toolCallId: "pi-search",
        toolName: "mcp",
        args: { tool: "search_tools", args: "{\"query\":\"context7\"}" },
      }),
      JSON.stringify({
        type: "tool_execution_end",
        toolCallId: "pi-search",
        toolName: "mcp",
        result: { content: [] },
        isError: false,
      }),
      JSON.stringify({
        type: "tool_use",
        part: {
          tool: "oceans_call_tool",
          state: { status: "completed", input: { name: "context7.query-docs" } },
        },
      }),
    ].join("\n");

    expect(parseToolCalls(output)).toEqual([
      {
        name: "mcp",
        input: { tool: "search_tools", args: "{\"query\":\"context7\"}" },
        status: "completed",
      },
      {
        name: "oceans_call_tool",
        input: { name: "context7.query-docs" },
        status: "completed",
      },
    ]);
  });

  test("preserves failed and unfinished tool statuses", () => {
    const output = [
      JSON.stringify({
        type: "tool_execution_start",
        toolCallId: "pi-failed",
        toolName: "mcp",
        args: { tool: "call_tool" },
      }),
      JSON.stringify({
        type: "tool_execution_end",
        toolCallId: "pi-failed",
        toolName: "mcp",
        result: { content: [] },
        isError: true,
      }),
      JSON.stringify({
        type: "tool_execution_start",
        toolCallId: "pi-unfinished",
        toolName: "read",
        args: { path: "source.txt" },
      }),
      JSON.stringify({
        type: "tool_use",
        part: {
          tool: "oceans_call_tool",
          state: { status: "error", input: { name: "context7.query-docs" } },
        },
      }),
    ].join("\n");

    expect(parseToolCalls(output).map(({ name, status }) => ({ name, status }))).toEqual([
      { name: "mcp", status: "error" },
      { name: "read", status: "started" },
      { name: "oceans_call_tool", status: "error" },
    ]);
  });
});
