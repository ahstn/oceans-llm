import { randomUUID } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { runCommand } from "../process.js";
import type { GatewayRuntime, HarnessAdapter, HarnessRun } from "../types.js";
import { parseAssistantOutput, parseToolCalls } from "./events.js";
import { createHarnessEnvironment, createIsolatedPaths } from "./isolation.js";

const PACKAGE_ROOT = resolve(fileURLToPath(new URL("../..", import.meta.url)));
const OPENCODE_BINARY = join(PACKAGE_ROOT, "node_modules", ".bin", "opencode");

export class OpenCodeAdapter implements HarnessAdapter {
  readonly key = "opencode" as const;
  readonly label = "OpenCode";
  readonly #runtime: GatewayRuntime;

  constructor(runtime: GatewayRuntime) {
    this.#runtime = runtime;
  }

  async run(workspace: string, prompt: string): Promise<HarnessRun> {
    const requestTag = randomUUID();
    const isolated = await createIsolatedPaths(workspace, this.key);
    const pluginDirectory = join(isolated.config, "plugins");
    await mkdir(pluginDirectory, { recursive: true });
    await writeFile(join(pluginDirectory, "oceans-guardrails.js"), guardrailPluginSource(), "utf8");
    const config = {
      agent: {
        build: {
          steps: 8,
        },
      },
      mcp: {
        oceans: {
          enabled: true,
          headers: { Authorization: "Bearer {env:OCEANS_API_KEY}" },
          oauth: false,
          timeout: 60_000,
          type: "remote",
          url: `${this.#runtime.baseUrl}/mcp`,
        },
      },
      permission: {
        bash: "allow",
        external_directory: "deny",
        task: "deny",
      },
      model: `oceans/${this.#runtime.model}`,
      provider: {
        oceans: {
          models: {
            [this.#runtime.model]: {
              limit: { context: 128_000, output: 8_192 },
              name: "Oceans OpenRouter Harness Model",
            },
          },
          name: "Oceans LLM Gateway",
          npm: "@ai-sdk/openai-compatible",
          options: {
            apiKey: "{env:OCEANS_API_KEY}",
            baseURL: `${this.#runtime.baseUrl}/v1`,
            headers: { "x-oceans-tags": `harness_run=${requestTag}` },
          },
        },
      },
    };
    const environment = createHarnessEnvironment(isolated, {
      OCEANS_API_KEY: this.#runtime.apiKey,
      OCEANS_BASE_URL: this.#runtime.baseUrl,
      OPENCODE_CONFIG_CONTENT: JSON.stringify(config),
      OPENCODE_CONFIG_DIR: isolated.config,
      OPENCODE_DISABLE_AUTOUPDATE: "true",
      OPENCODE_DISABLE_DEFAULT_PLUGINS: "true",
      OPENCODE_DISABLE_LSP_DOWNLOAD: "true",
    });
    const mcpStatus = await runCommand(
      OPENCODE_BINARY,
      ["--print-logs", "--log-level", "DEBUG", "mcp", "list"],
      {
        cwd: workspace,
        env: environment,
        timeoutMs: 30_000,
      },
    );
    const mcpStatusLine = mcpStatus.stdout
      .split("\n")
      .map(stripAnsi)
      .find((line) => line.includes("oceans"));
    if (!mcpStatusLine || !/\boceans\s+connected\b/i.test(mcpStatusLine)) {
      throw new Error(
        `OpenCode did not connect to Oceans MCP:\n${mcpStatus.stdout}\n${mcpStatus.stderr}`,
      );
    }
    const result = await runCommand(
      OPENCODE_BINARY,
      [
        "run",
        "--format",
        "json",
        "--model",
        `oceans/${this.#runtime.model}`,
        "--dir",
        workspace,
        "--auto",
        "--",
        prompt,
      ],
      {
        cwd: workspace,
        env: environment,
        timeoutMs: 90_000,
      },
    );
    return {
      output: parseAssistantOutput(result.stdout),
      requestTag,
      toolCalls: parseToolCalls(result.stdout),
    };
  }
}

export function guardrailPluginSource(): string {
  return `
const guardrailUrl = new URL("/api/v1/guardrails/evaluate", process.env.OCEANS_BASE_URL).toString();
const guardrailTimeoutMs = Number(process.env.OCEANS_GUARDRAIL_TIMEOUT_MS ?? "2000");

export const OceansGuardrails = async () => ({
  "tool.execute.before": async (input, output) => {
    if (input.tool !== "bash") return;
    const command = output.args?.command;
    if (typeof command !== "string") {
      throw new Error("Shell command is missing");
    }
    const response = await fetch(guardrailUrl, {
      signal: AbortSignal.timeout(guardrailTimeoutMs),
      method: "POST",
      headers: {
        authorization: "Bearer " + process.env.OCEANS_API_KEY,
        "content-type": "application/json",
      },
      body: JSON.stringify({ tool_name: "bash", command }),
    });
    if (!response.ok) {
      throw new Error("Guardrail evaluation failed with HTTP " + response.status);
    }
    const decision = await response.json();
    console.error("oceans_guardrail_decision_id=" + decision.decision_id);
    if (!decision.allowed) {
      throw new Error("Oceans guardrail denied shell execution: " + decision.reason_code);
    }
    return;
  },
});
`;
}

function stripAnsi(value: string): string {
  return value.replace(/\u001B\[[0-?]*[ -/]*[@-~]/g, "");
}
