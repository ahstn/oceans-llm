import { randomUUID } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { runCommand } from "../process.js";
import type { GatewayRuntime, HarnessAdapter, HarnessRun } from "../types.js";
import { parseAssistantOutput, parseToolCalls } from "./events.js";
import { createHarnessEnvironment, createIsolatedPaths } from "./isolation.js";

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
    const requestTag = randomUUID();
    const isolated = await createIsolatedPaths(workspace, this.key);
    const agentDir = join(isolated.config, "agent");
    await mkdir(agentDir, { recursive: true });
    const sandboxExtension = join(agentDir, "workspace-sandbox.mjs");
    await Promise.all([
      writeFile(
        join(agentDir, "models.json"),
        JSON.stringify({
          providers: {
            oceans: {
              api: "openai-completions",
              apiKey: "$OCEANS_API_KEY",
              baseUrl: `${this.#runtime.baseUrl}/v1`,
              headers: { "x-oceans-tags": `harness_run=${requestTag}` },
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
      writeFile(sandboxExtension, workspaceSandboxSource(workspace), "utf8"),
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
        "--extension",
        sandboxExtension,
        "--provider",
        "oceans",
        "--model",
        this.#runtime.model,
        "--thinking",
        "off",
        "--approve",
      ],
      {
        cwd: workspace,
        env: createHarnessEnvironment(isolated, {
          OCEANS_API_KEY: this.#runtime.apiKey,
          PI_CODING_AGENT_DIR: agentDir,
        }),
        stdin: prompt,
        timeoutMs: 180_000,
      },
    );
    return {
      output: parseAssistantOutput(result.stdout),
      requestTag,
      toolCalls: parseToolCalls(result.stdout),
    };
  }
}

function workspaceSandboxSource(workspace: string): string {
  return `import { isAbsolute, relative, resolve, sep } from "node:path";

const workspace = ${JSON.stringify(workspace)};

function isWithinWorkspace(target) {
  const relativePath = relative(workspace, resolve(workspace, target));
  return (
    relativePath === "" ||
    (relativePath !== ".." && !relativePath.startsWith(".." + sep) && !isAbsolute(relativePath))
  );
}

export default function workspaceSandbox(pi) {
  pi.on("tool_call", async (event) => {
    if (event.toolName === "bash") {
      return { block: true, reason: "Shell access is disabled in the harness integration sandbox" };
    }
    if (!["edit", "read", "write"].includes(event.toolName)) {
      return;
    }
    const target = event.input?.path;
    if (typeof target !== "string" || !isWithinWorkspace(target)) {
      return { block: true, reason: "File access outside the harness workspace is disabled" };
    }
  });
}
`;
}
