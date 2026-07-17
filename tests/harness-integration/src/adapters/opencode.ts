import { randomUUID } from "node:crypto";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { runCommand } from "../process.js";
import type { GatewayRuntime, HarnessAdapter, HarnessRun } from "../types.js";
import { parseToolCalls } from "./events.js";
import { createIsolatedPaths } from "./isolation.js";

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
    const environment: NodeJS.ProcessEnv = {
      HOME: isolated.home,
      OCEANS_API_KEY: this.#runtime.apiKey,
      OPENCODE_CONFIG_CONTENT: JSON.stringify(config),
      OPENCODE_CONFIG_DIR: isolated.config,
      OPENCODE_DISABLE_DEFAULT_PLUGINS: "true",
      OPENCODE_DISABLE_LSP_DOWNLOAD: "true",
      OPENCODE_DISABLE_UPDATE_CHECK: "true",
      XDG_CACHE_HOME: isolated.cache,
      XDG_CONFIG_HOME: isolated.config,
      XDG_DATA_HOME: isolated.data,
    };
    const mcpStatus = await runCommand(
      OPENCODE_BINARY,
      ["--print-logs", "--log-level", "DEBUG", "mcp", "list"],
      {
      cwd: workspace,
      env: environment,
      timeoutMs: 30_000,
      },
    );
    if (!mcpStatus.stdout.includes("oceans") || /failed|error/i.test(mcpStatus.stdout)) {
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
    const output = `${result.stdout}\n${result.stderr}`.trim();
    return { output, requestTag, toolCalls: parseToolCalls(output) };
  }
}
