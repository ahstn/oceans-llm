import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { afterAll, beforeAll, describe, inject, test, type ExpectStatic } from "vitest";

import { ClaudeCodeAdapter } from "./adapters/claude-code.js";
import { OmpAdapter } from "./adapters/omp.js";
import { OpenCodePluginAdapter } from "./adapters/opencode-plugin.js";
import { PiAdapter } from "./adapters/pi.js";
import { GatewayAdminClient } from "./gateway-client.js";
import {
  describeCell,
  MATRIX,
  MATRIX_MODELS,
  MATRIX_PROVIDERS,
  MATRIX_TESTS,
  runnable,
  type MatrixHarness,
  type MatrixTest,
} from "./matrix.js";
import { expectRequestLogs, reportRun, withWorkspace } from "./run-checks.js";
import type { HarnessAdapter, HarnessKey, HarnessRun, Operation, Provider } from "./types.js";

const runtime = inject("gateway");
const openCode = new OpenCodePluginAdapter(runtime);
const adapters: Record<HarnessKey, HarnessAdapter> = {
  claude_cli: new ClaudeCodeAdapter(runtime),
  oh_my_pi: new OmpAdapter(runtime),
  opencode: openCode,
  pi: new PiAdapter(runtime),
};
const admin = new GatewayAdminClient(runtime);

beforeAll(() => admin.login());
afterAll(() => openCode.close());

for (const row of MATRIX) {
  const adapter = adapters[row.harness];
  // The three models of one provider and test run at the same time; `maxConcurrency` limits the
  // file to three.
  describe.concurrent(row.label, () => {
    for (const provider of MATRIX_PROVIDERS) {
      for (const kind of MATRIX_TESTS) {
        for (const family of MATRIX_MODELS) {
          const cell = row.cells[provider][family];
          const plan = runnable(cell);
          // A blocked or not-applicable cell is data about the harness, not a skipped test.
          if (plan === undefined) {
            if (kind === "chat") {
              process.stdout.write(
                `Matrix cell: ${row.harness} ${provider} ${family}: ${describeCell(cell)}\n`,
              );
            }
            continue;
          }
          test(`${provider} ${family} ${kind}`, (context) => {
            if (plan.optInEnv !== undefined && !process.env[plan.optInEnv]) {
              context.skip(describeCell(cell));
              return;
            }
            const model = modelKey(provider, family);
            if (model === undefined) {
              context.skip(`${provider} is not configured for this gateway: ${PROVIDER_SETUP[provider]}`);
              return;
            }
            return withWorkspace(`matrix-${row.harness}`, (workspace) =>
              runMatrixTest(context.expect, row, adapter, kind, workspace, provider, model, plan.operation),
            );
          });
        }
      }
    }
  });
}

/** How to configure each provider for the managed gateway. */
const PROVIDER_SETUP: Record<Provider, string> = {
  openrouter: "set OPENROUTER_API_KEY",
  vertex: "set OCEANS_HARNESS_VERTEX_PROJECT and GOOGLE_APPLICATION_CREDENTIALS",
  bedrock: "set OCEANS_HARNESS_BEDROCK=1 and an AWS region, with AWS credentials in the default chain",
};

function modelKey(provider: Provider, family: (typeof MATRIX_MODELS)[number]): string | undefined {
  return provider === "openrouter"
    ? runtime.pluginModels[family]
    : runtime.providerModels[provider]?.[family];
}

async function runMatrixTest(
  expect: ExpectStatic,
  row: MatrixHarness,
  adapter: HarnessAdapter,
  kind: MatrixTest,
  workspace: string,
  provider: Provider,
  model: string,
  operation: Operation,
): Promise<void> {
  let run: HarnessRun;
  if (kind === "chat") {
    const token = `OCEANS_MATRIX_${row.harness.toUpperCase()}_OK`;
    run = await adapter.run(workspace, `Reply with exactly ${token} and no other text.`, model);
    expect(run.output).toBe(token);
  } else if (kind === "files") {
    const marker = `matrix-${row.harness}-${Date.now()}`;
    await writeFile(join(workspace, "source.txt"), marker, "utf8");
    run = await adapter.run(
      workspace,
      `Use the ${adapter.tools.read} tool to read source.txt. Then use the ${adapter.tools.write.join(" or ")} tool, not a shell command, to write destination.txt containing exactly ${marker}-copied. You must use both tools.`,
      model,
    );
    const completed = run.toolCalls
      .filter((toolCall) => toolCall.status === "completed")
      .map((toolCall) => toolCall.name);
    const evidence = JSON.stringify(run.toolCalls);
    expect(completed, evidence).toContain(adapter.tools.read);
    expect(completed.some((name) => adapter.tools.write.includes(name)), evidence).toBe(true);
    // A patch tool ends the file with a line break.
    expect((await readFile(join(workspace, "destination.txt"), "utf8")).trimEnd()).toBe(
      `${marker}-copied`,
    );
  } else {
    const before = utcDate();
    run = await adapter.run(
      workspace,
      `Use the ${adapter.tools.shell} tool to run exactly this command: date -u +%F\nThen reply with only the date that the command printed.`,
      model,
    );
    const dates = [before, utcDate()];
    const shellCall = run.toolCalls.find(
      (toolCall) =>
        toolCall.name === adapter.tools.shell &&
        toolCall.status === "completed" &&
        String((toolCall.input as { command?: unknown } | undefined)?.command).includes("date"),
    );
    expect(shellCall, JSON.stringify(run.toolCalls)).toBeDefined();
    expect(dates.some((date) => run.output.includes(date)), run.output).toBe(true);
  }
  const logs = await expectRequestLogs(
    expect,
    admin,
    run,
    // Managed provider ids are the provider names.
    { harness: row.harness, model, operation, provider },
    kind === "chat" ? 1 : 2,
  );
  reportRun(`${row.harness} ${model} ${kind}`, run, logs);
}

function utcDate(): string {
  return new Date().toISOString().slice(0, 10);
}
