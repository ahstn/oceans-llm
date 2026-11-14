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

  test("treats an OMP tool end without isError as completed", () => {
    const output = [
      JSON.stringify({
        type: "tool_execution_start",
        toolCallId: "omp-bash",
        toolName: "bash",
        args: { command: "bwrap --die-with-parent /bin/sh -c 'date -u +%F'" },
      }),
      JSON.stringify({
        type: "tool_execution_end",
        toolCallId: "omp-bash",
        toolName: "bash",
        result: { content: [{ type: "text", text: "2026-11-13" }] },
      }),
      JSON.stringify({
        type: "agent_end",
        messages: [
          { role: "assistant", content: [{ type: "text", text: "2026-11-13" }], stopReason: "stop" },
        ],
      }),
    ].join("\n");

    expect(parseToolCalls(output)).toEqual([
      {
        name: "bash",
        input: { command: "bwrap --die-with-parent /bin/sh -c 'date -u +%F'" },
        status: "completed",
      },
    ]);
    expect(parseAssistantOutput(output)).toBe("2026-11-13");
  });

  test("reads Claude Code stream-json tool calls, results and the final result", () => {
    const output = [
      JSON.stringify({ type: "system", subtype: "init", tools: ["Read", "Write"] }),
      JSON.stringify({
        type: "assistant",
        message: {
          content: [
            { type: "text", text: "Reading the file." },
            { type: "tool_use", id: "toolu_read", name: "Read", input: { file_path: "/w/source.txt" } },
            { type: "tool_use", id: "toolu_denied", name: "Write", input: { file_path: "/etc/x" } },
          ],
        },
      }),
      JSON.stringify({
        type: "user",
        message: {
          content: [
            { type: "tool_result", tool_use_id: "toolu_read", content: "marker" },
            { type: "tool_result", tool_use_id: "toolu_denied", content: "denied", is_error: true },
          ],
        },
      }),
      JSON.stringify({ type: "result", subtype: "success", is_error: false, result: " DONE\n" }),
    ].join("\n");

    expect(parseToolCalls(output)).toEqual([
      { name: "Read", input: { file_path: "/w/source.txt" }, status: "completed" },
      { name: "Write", input: { file_path: "/etc/x" }, status: "error" },
    ]);
    expect(parseAssistantOutput(output)).toBe("DONE");
  });

  test("rejects a Claude Code error result", () => {
    for (const result of [
      { type: "result", subtype: "error_max_turns", is_error: true },
      { type: "result", subtype: "success", is_error: true, result: "API Error: 400" },
    ]) {
      expect(() => parseAssistantOutput(JSON.stringify(result))).toThrow(/agent error/);
    }
  });
});
