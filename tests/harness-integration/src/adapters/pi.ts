import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { runCommand } from "../process.js";
import type { GatewayRuntime, HarnessAdapter, HarnessRun } from "../types.js";
import { parseToolCalls } from "./events.js";
import { createIsolatedPaths } from "./isolation.js";

const PACKAGE_ROOT = resolve(fileURLToPath(new URL("../..", import.meta.url)));
const PI_BINARY = join(PACKAGE_ROOT, "node_modules", ".bin", "pi");
const MCP_EXTENSION = join(PACKAGE_ROOT, "node_modules", "pi-mcp-adapter", "index.ts");

export class PiAdapter implements HarnessAdapter {
  readonly key = "pi" as const;
  readonly label = "Pi";
  readonly #runtime: GatewayRuntime;

  constructor(runtime: GatewayRuntime) {
    this.#runtime = runtime;
  }

  async run(workspace: string, prompt: string): Promise<HarnessRun> {
    const isolated = await createIsolatedPaths(workspace, this.key);
    const agentDir = join(isolated.config, "agent");
    await mkdir(agentDir, { recursive: true });
    await Promise.all([
      writeFile(
        join(agentDir, "models.json"),
        JSON.stringify({
          providers: {
            oceans: {
              api: "openai-completions",
              apiKey: "$OCEANS_API_KEY",
              baseUrl: `${this.#runtime.baseUrl}/v1`,
              models: [
                {
                  contextWindow: 128_000,
                  id: this.#runtime.model,
                  input: ["text"],
                  maxTokens: 8_192,
                  name: "Oceans OpenRouter Harness Model",
                  reasoning: false,
                },
              ],
            },
          },
        }),
        "utf8",
      ),
      writeFile(
        join(workspace, ".mcp.json"),
        JSON.stringify({
          mcpServers: {
            oceans: {
              headers: { Authorization: "Bearer ${OCEANS_API_KEY}" },
              lifecycle: "eager",
              requestTimeoutMs: 60_000,
              url: `${this.#runtime.baseUrl}/mcp`,
            },
          },
        }),
        "utf8",
      ),
    ]);

    const result = await runCommand(
      PI_BINARY,
      [
        "--mode",
        "json",
        "--no-session",
        "--no-context-files",
        "--no-skills",
        "--no-prompt-templates",
        "--no-extensions",
        "--extension",
        MCP_EXTENSION,
        "--provider",
        "oceans",
        "--model",
        this.#runtime.model,
        "--api-key",
        this.#runtime.apiKey,
        "--thinking",
        "off",
        "--approve",
        prompt,
      ],
      {
        cwd: workspace,
        env: {
          HOME: isolated.home,
          OCEANS_API_KEY: this.#runtime.apiKey,
          PI_CODING_AGENT_DIR: agentDir,
          XDG_CACHE_HOME: isolated.cache,
          XDG_CONFIG_HOME: isolated.config,
          XDG_DATA_HOME: isolated.data,
        },
        timeoutMs: 180_000,
      },
    );
    const output = `${result.stdout}\n${result.stderr}`.trim();
    return { output, toolCalls: parseToolCalls(output) };
  }
}
