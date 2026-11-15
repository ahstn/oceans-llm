import { afterAll, beforeAll, describe, inject, test } from "vitest";
import { z } from "zod";

import { ClaudeCodeAdapter } from "./adapters/claude-code.js";
import { OmpAdapter } from "./adapters/omp.js";
import { fetchGeneratedOpenCodeConfig, OpenCodeAdapter } from "./adapters/opencode.js";
import { OpenCodePluginAdapter } from "./adapters/opencode-plugin.js";
import { parseGeneratedPiConfig, PiAdapter } from "./adapters/pi.js";
import { fetchGeneratedClientConfig, GatewayAdminClient } from "./gateway-client.js";
import { MATRIX, runnable } from "./matrix.js";
import { expectRequestLogs, reportRun, withWorkspace } from "./run-checks.js";
import type { HarnessAdapter, HarnessKey, Operation, ToolCall } from "./types.js";

const runtime = inject("gateway");
const openCode = new OpenCodePluginAdapter(runtime);
const adapters: Record<HarnessKey, HarnessAdapter> = {
  claude_cli: new ClaudeCodeAdapter(runtime),
  oh_my_pi: new OmpAdapter(runtime),
  opencode: openCode,
  pi: new PiAdapter(runtime),
};
const admin = new GatewayAdminClient(runtime);

// The gateway API that each generated client config sends requests to.
const OPERATION_BY_OPENCODE_PACKAGE: Record<string, Operation> = {
  "@opencode/ai/providers/anthropic-compatible": "messages",
  "@opencode/ai/providers/openai-compatible": "chat_completions",
  "@opencode/ai/providers/openai-compatible/responses": "responses",
};
const OPERATION_BY_PI_API: Record<string, Operation> = {
  "anthropic-messages": "messages",
  "openai-completions": "chat_completions",
  "openai-responses": "responses",
};
// Pi's MCP adapter has one `mcp` tool, and the input names the server tool to call.
const PiMcpCallSchema = z.object({ tool: z.string() });

beforeAll(() => admin.login());
afterAll(() => openCode.close());

describe("Context7 through Oceans aggregate MCP", () => {
  for (const row of MATRIX) {
    const plan = runnable(row.cells.openrouter[row.mcpModel]);
    if (plan === undefined) {
      throw new Error(`${row.label} has no runnable OpenRouter MCP model`);
    }
    const model = runtime.pluginModels[row.mcpModel];
    test(`${row.label}: ${model} proxies Context7 tools`, ({ expect }) =>
      withWorkspace(`mcp-${row.harness}`, async (workspace) => {
        const run = await adapters[row.harness].run(
          workspace,
          "Use the configured Oceans MCP server and perform exactly these operations in order before answering: first invoke the Oceans aggregate tool named search_tools with a query for Context7 library-documentation tools; then use that result to invoke the aggregate tool named call_tool and ask Context7 for Vitest documentation about advancing fake timers. Both aggregate tool calls are mandatory. After the MCP request responds, briefly acknowledge completion.",
          model,
          { mcp: true },
        );

        const discoveryIndex = run.toolCalls.findIndex(
          (toolCall) => aggregateOperation(toolCall) === "search_tools",
        );
        // A model can retry a Context7 call with corrected arguments, so take the first call that
        // completed.
        const documentationIndex = run.toolCalls.findIndex(
          (toolCall, index) =>
            index > discoveryIndex &&
            aggregateOperation(toolCall) === "call_tool" &&
            toolCall.status === "completed" &&
            JSON.stringify(toolCall.input).toLowerCase().includes("context7"),
        );
        const evidence = JSON.stringify(run.toolCalls);
        expect(discoveryIndex, evidence).toBeGreaterThanOrEqual(0);
        expect(documentationIndex, evidence).toBeGreaterThan(discoveryIndex);
        expect(run.toolCalls[discoveryIndex]?.status, evidence).toBe("completed");
        // This verifies successful aggregate MCP proxying, not the nondeterministic Context7
        // payload or model synthesis. Completion comes from each harness's terminal tool event,
        // while `run` separately requires a successful, non-empty final assistant response.
        const logs = await expectRequestLogs(
          expect,
          admin,
          run,
          { harness: row.harness, model, operation: plan.operation, provider: "openrouter" },
          2,
        );
        reportRun(`${row.harness} ${model} mcp`, run, logs);
      }));
  }
});

test("OpenCode with the generated snippet answers through Oceans", ({ expect }) =>
  withWorkspace("opencode-snippet", async (workspace) => {
    const model = runtime.pluginModels.openai;
    const config = await fetchGeneratedOpenCodeConfig(runtime, model);
    const run = await new OpenCodeAdapter(runtime).run(
      workspace,
      "Reply with exactly OCEANS_SNIPPET_OK and no other text.",
      model,
    );

    expect(run.output).toBe("OCEANS_SNIPPET_OK");
    const packages = Object.values(config.providers).map((provider) => provider.package);
    const operation = OPERATION_BY_OPENCODE_PACKAGE[packages[0] ?? ""];
    expect(packages, "generated OpenCode provider packages").toHaveLength(1);
    expect(operation, `unknown OpenCode provider package ${packages[0]}`).toBeDefined();
    const logs = await expectRequestLogs(
      expect,
      admin,
      run,
      { harness: "opencode", model, operation: operation!, provider: "openrouter" },
      1,
    );
    reportRun(`opencode-snippet ${model} chat`, run, logs);
  }));

const allowlistedUser = runtime.allowlistedUser;
test.skipIf(!allowlistedUser)(
  "Pi allows a user whose normalized email is in the model allowlist",
  ({ expect }) =>
    withWorkspace("pi-allowlist", async (workspace) => {
      if (!allowlistedUser) {
        throw new Error("Allowlisted user runtime is not configured");
      }
      const { model } = allowlistedUser;
      const [content] = await fetchGeneratedClientConfig(runtime, model, "pi");
      const { provider } = parseGeneratedPiConfig(JSON.parse(content ?? "null"), model);
      const run = await new PiAdapter({ ...runtime, apiKey: allowlistedUser.apiKey }).run(
        workspace,
        "Reply with exactly ALLOWLIST_AUTHZ_OK and no other text.",
        model,
      );

      expect(run.output).toBe("ALLOWLIST_AUTHZ_OK");
      const operation = OPERATION_BY_PI_API[provider.api];
      expect(operation, `unknown Pi api ${provider.api}`).toBeDefined();
      const logs = await expectRequestLogs(
        expect,
        admin,
        run,
        { harness: "pi", model, operation: operation!, provider: "openrouter" },
        1,
      );
      reportRun(`pi-allowlist ${model} chat`, run, logs);
    }),
);

/** The Oceans aggregate tool that a call invoked, whatever the harness names the tool. */
function aggregateOperation(toolCall: ToolCall): string | undefined {
  const name =
    toolCall.name === "mcp" ? PiMcpCallSchema.safeParse(toolCall.input).data?.tool : toolCall.name;
  return ["search_tools", "call_tool"].find((operation) => name?.endsWith(operation));
}
