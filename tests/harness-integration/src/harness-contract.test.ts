import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeAll, beforeEach, describe, expect, inject, test } from "vitest";

import { OpenCodeAdapter } from "./adapters/opencode.js";
import { PiAdapter } from "./adapters/pi.js";
import { GatewayAdminClient } from "./gateway-client.js";
import type { GatewayRuntime, HarnessAdapter } from "./types.js";

const runtime = inject("gateway");
const adapters: HarnessAdapter[] = [new PiAdapter(runtime), new OpenCodeAdapter(runtime)];

for (const adapter of adapters) {
  defineHarnessContract(adapter, runtime);
}

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
      const previousLogIds = await admin.requestLogIds();
      const result = await adapter.run(
        workspace,
        "Reply with exactly OCEANS_HARNESS_OK and no other text.",
      );

      expect(result.output).toContain("OCEANS_HARNESS_OK");
      const requestLog = await admin.waitForSuccessfulModelLog(previousLogIds);
      expect(requestLog.provider_key).toBe("openrouter");
    });

    test("reads and writes workspace files with normal harness tools", async () => {
      const marker = `harness-${adapter.key}-${Date.now()}`;
      await writeFile(join(workspace, "source.txt"), marker, "utf8");

      const result = await adapter.run(
        workspace,
        `Use the read tool to read source.txt. Then use the write tool, not a shell command, to write destination.txt containing exactly ${marker}-copied. You must use both tools.`,
      );

      expect(result.toolCalls).toContain("read");
      expect(result.toolCalls).toContain("write");
      await expect(readFile(join(workspace, "destination.txt"), "utf8")).resolves.toBe(
        `${marker}-copied`,
      );
    });

    test("gets useful Vitest documentation from Context7 through aggregate MCP", async () => {
      const result = await adapter.run(
        workspace,
        "Use the configured Oceans MCP server. You must invoke its MCP tools to search for Context7 library-documentation tools, then call Context7 for Vitest fake-timer documentation. Summarize the tool-backed answer and include the words VITEST and TIMER.",
      );

      expect(
        result.toolCalls.some(
          (tool) => tool === "mcp" || tool.endsWith("search_tools") || tool.endsWith("call_tool"),
        ),
        result.output,
      ).toBe(true);
      expect(result.output).toMatch(/vitest/i);
      expect(result.output).toMatch(/timer/i);
    });
  });
}
