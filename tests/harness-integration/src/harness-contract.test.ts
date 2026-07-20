import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeAll, beforeEach, describe, expect, inject, test } from "vitest";

import { OpenCodeAdapter } from "./adapters/opencode.js";
import { PiAdapter } from "./adapters/pi.js";
import { GatewayAdminClient } from "./gateway-client.js";
import type { GatewayRuntime, HarnessAdapter, ToolCall } from "./types.js";

const runtime = inject("gateway");
const adapters: HarnessAdapter[] = [new PiAdapter(runtime), new OpenCodeAdapter(runtime)];

for (const adapter of adapters) {
  defineHarnessContract(adapter, runtime);
}

const allowlistedUser = runtime.allowlistedUser;
describe("Pi human-user model allowlist contract", () => {
  let workspace: string;

  beforeEach(async () => {
    workspace = await mkdtemp(join(tmpdir(), "oceans-pi-allowlist-"));
  });
  afterEach(async () => rm(workspace, { force: true, recursive: true }));

  test.skipIf(!allowlistedUser)(
    "allows a user whose normalized email is in the model allowlist",
    async () => {
      if (!allowlistedUser) {
        throw new Error("Allowlisted user runtime is not configured");
      }
      const adapter = new PiAdapter({
        ...runtime,
        apiKey: allowlistedUser.apiKey,
        model: allowlistedUser.model,
      });
      const result = await adapter.run(
        workspace,
        "Reply with exactly ALLOWLIST_AUTHZ_OK and no other text.",
      );

      expect(result.output).toBe("ALLOWLIST_AUTHZ_OK");
      const admin = new GatewayAdminClient({
        ...runtime,
        model: allowlistedUser.model,
      });
      await admin.login();
      const requestLog = await admin.waitForSuccessfulModelLog(result.requestTag);
      expect(requestLog.provider_key).toBe("openrouter");
    },
  );
});

function defineHarnessContract(adapter: HarnessAdapter, gateway: GatewayRuntime): void {
  describe(`${adapter.label} native harness contract`, () => {
    const admin = new GatewayAdminClient(gateway);
    let workspace: string;

    beforeAll(async () => admin.login());
    beforeEach(async () => {
      workspace = await mkdtemp(join(tmpdir(), `oceans-${adapter.key}-`));
    });
    afterEach(async () => rm(workspace, { force: true, recursive: true }));

    test("routes an OpenRouter model call through Oceans and records it", async () => {
      const result = await adapter.run(
        workspace,
        "Reply with exactly OCEANS_HARNESS_OK and no other text.",
      );

      expect(result.output).toContain("OCEANS_HARNESS_OK");
      const requestLog = await admin.waitForSuccessfulModelLog(result.requestTag);
      expect(requestLog.provider_key).toBe("openrouter");
    });

    test("reads and writes workspace files with normal harness tools", async () => {
      const marker = `harness-${adapter.key}-${Date.now()}`;
      await writeFile(join(workspace, "source.txt"), marker, "utf8");

      const result = await adapter.run(
        workspace,
        `Use the read tool to read source.txt. Then use the write tool, not a shell command, to write destination.txt containing exactly ${marker}-copied. You must use both tools.`,
      );

      expect(result.toolCalls.map((toolCall) => toolCall.name)).toContain("read");
      expect(result.toolCalls.map((toolCall) => toolCall.name)).toContain("write");
      await expect(readFile(join(workspace, "destination.txt"), "utf8")).resolves.toBe(
        `${marker}-copied`,
      );
    });

    test("proxies Context7 tools through aggregate MCP", async () => {
      const result = await adapter.run(
        workspace,
        "Use the configured Oceans MCP server and perform exactly these operations in order before answering: first invoke the Oceans aggregate tool named search_tools with a query for Context7 library-documentation tools; then use that result to invoke the aggregate tool named call_tool and ask Context7 for Vitest documentation about advancing fake timers. Both aggregate tool calls are mandatory. After the MCP request responds, briefly acknowledge completion.",
      );

      const discoveryIndex = result.toolCalls.findIndex(
        (toolCall) => aggregateOperation(toolCall) === "search_tools",
      );
      const documentationIndex = result.toolCalls.findIndex(
        (toolCall, index) =>
          index > discoveryIndex &&
          aggregateOperation(toolCall) === "call_tool" &&
          toolInputContains(toolCall, "context7"),
      );
      const toolEvidence = JSON.stringify(result.toolCalls);
      expect(discoveryIndex, toolEvidence).toBeGreaterThanOrEqual(0);
      expect(documentationIndex, toolEvidence).toBeGreaterThan(discoveryIndex);
      expect(result.toolCalls[discoveryIndex]?.status, toolEvidence).toBe("completed");
      expect(result.toolCalls[documentationIndex]?.status, toolEvidence).toBe("completed");

      // This contract verifies successful aggregate MCP proxying, not the nondeterministic Context7
      // payload or model synthesis. Completion comes from each harness's terminal tool event, while
      // adapter.run separately requires a successful, non-empty final assistant response.
    });
  });
}

function aggregateOperation(toolCall: ToolCall): string | undefined {
  if (toolCall.name.endsWith("search_tools")) {
    return "search_tools";
  }
  if (toolCall.name.endsWith("call_tool")) {
    return "call_tool";
  }
  if (toolCall.name !== "mcp" || !isRecord(toolCall.input)) {
    return undefined;
  }
  const operation = toolCall.input.tool;
  if (typeof operation !== "string") {
    return undefined;
  }
  if (operation.endsWith("search_tools")) {
    return "search_tools";
  }
  return operation.endsWith("call_tool") ? "call_tool" : undefined;
}

function toolInputContains(toolCall: ToolCall, value: string): boolean {
  return JSON.stringify(toolCall.input).toLowerCase().includes(value.toLowerCase());
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
