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
              api: "openai-responses",
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
          OCEANS_BASE_URL: this.#runtime.baseUrl,
          PI_OFFLINE: "1",
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

export function workspaceSandboxSource(workspace: string): string {
  return `import { isAbsolute, relative, resolve, sep } from "node:path";

const workspace = ${JSON.stringify(workspace)};
const guardrailUrl = new URL("/api/v1/guardrails/evaluate", process.env.OCEANS_BASE_URL).toString();
const guardrailTimeoutMs = Number(process.env.OCEANS_GUARDRAIL_TIMEOUT_MS ?? "2000");

function validateGuardrailDecision(value) {
  if (
    value === null ||
    typeof value !== "object" ||
    typeof value.allowed !== "boolean" ||
    typeof value.transformed !== "boolean" ||
    typeof value.decision_id !== "string" ||
    value.decision_id.length === 0 ||
    (!value.allowed && (typeof value.reason_code !== "string" || value.reason_code.length === 0))
  ) {
    throw new Error("Guardrail evaluation returned an invalid response");
  }
  return value;
}

async function guardShell(command) {
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
  return validateGuardrailDecision(await response.json());
}


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
      const command = event.input?.command;
      if (typeof command !== "string") {
        return { block: true, reason: "Shell command is missing" };
      }
      let decision;
      try {
        decision = await guardShell(command);
      } catch (error) {
        return { block: true, reason: String(error) };
      }
      console.error("oceans_guardrail_decision_id=" + decision.decision_id);
      if (!decision.allowed) {
        return {
          block: true,
          reason: "Oceans guardrail denied shell execution: " + decision.reason_code,
        };
      }
      if (decision.transformed) {
        if (typeof decision.output_command !== "string") {
          return { block: true, reason: "Oceans guardrail returned an invalid shell transformation" };
        }
        event.input.command = decision.output_command;
      }
      return;
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
