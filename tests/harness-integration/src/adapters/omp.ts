import { randomUUID } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { runCommand } from "../process.js";
import type { GatewayRuntime, HarnessAdapter, HarnessRun, HarnessRunOptions } from "../types.js";
import { parseAssistantOutput, parseToolCalls } from "./events.js";
import { createHarnessEnvironment, createIsolatedPaths, HARNESS_TIMEOUT_MS } from "./isolation.js";
import { sandboxHelpersSource } from "./sandbox.js";

const PACKAGE_ROOT = resolve(fileURLToPath(new URL("../..", import.meta.url)));
const OMP_BINARY = join(PACKAGE_ROOT, "node_modules", ".bin", "omp");
// OMP names an MCP tool `mcp__<server>_<tool>`. These are the Oceans aggregate tools.
const OCEANS_MCP_TOOLS = [
  "mcp__oceans_search_tools",
  "mcp__oceans_describe_tool",
  "mcp__oceans_call_tool",
];

/**
 * Oh-My-Pi with Oceans in `models.yml` and `litellm` discovery, as the OMP setup page describes.
 * OMP reads its config and state from `PI_CODING_AGENT_DIR` and the isolated HOME, so the
 * workspace never holds an `.omp` folder.
 */
export class OmpAdapter implements HarnessAdapter {
  readonly harness = "oh_my_pi" as const;
  readonly label = "OMP";
  readonly tools = { read: "read", shell: "bash", write: ["write"] };
  readonly #runtime: GatewayRuntime;

  constructor(runtime: GatewayRuntime) {
    this.#runtime = runtime;
  }

  async run(
    workspace: string,
    prompt: string,
    model: string,
    options: HarnessRunOptions = {},
  ): Promise<HarnessRun> {
    const requestTag = randomUUID();
    const isolated = await createIsolatedPaths(workspace, this.harness);
    const agentDir = join(isolated.config, "agent");
    await mkdir(agentDir, { recursive: true });
    const sandboxExtension = join(agentDir, "workspace-sandbox.mjs");
    const files = [
      // YAML accepts JSON, so models.yml is written as JSON.
      writeFile(
        join(agentDir, "models.yml"),
        JSON.stringify({
          providers: {
            oceans: {
              baseUrl: `${this.#runtime.baseUrl}/v1`,
              apiKey: "OCEANS_API_KEY",
              api: "openai-completions",
              // Without `auth: apiKey`, OMP gives custom anthropic-messages models Claude Code
              // OAuth request shaping.
              auth: "apiKey",
              headers: { "x-oceans-tags": `harness_run=${requestTag}` },
              discovery: { type: "litellm" },
              modelOverrides: Object.fromEntries(
                [
                  this.#runtime.pluginModels.anthropic,
                  ...Object.values(this.#runtime.providerModels).map((models) => models.anthropic),
                ]
                  .filter((model) => model !== undefined)
                  .map((model) => [model, { api: "anthropic-messages" }]),
              ),
            },
          },
        }),
        "utf8",
      ),
      writeFile(sandboxExtension, ompSandboxSource(workspace), "utf8"),
    ];
    if (options.mcp) {
      files.push(
        writeFile(
          join(agentDir, "mcp.json"),
          JSON.stringify({
            mcpServers: {
              oceans: {
                type: "http",
                url: `${this.#runtime.baseUrl}/mcp`,
                headers: { Authorization: "Bearer ${OCEANS_API_KEY}" },
                timeout: 60_000,
              },
            },
          }),
          "utf8",
        ),
      );
    }
    await Promise.all(files);

    const result = await runCommand(
      OMP_BINARY,
      [
        "--print",
        "--mode",
        "json",
        "--no-session",
        "--no-title",
        "--no-skills",
        "--no-rules",
        "--no-context-files",
        "--no-extensions",
        "--extension",
        sandboxExtension,
        "--no-lsp",
        "--no-pty",
        "--tools",
        ["read", "write", "bash", ...(options.mcp ? OCEANS_MCP_TOOLS : [])].join(","),
        "--model",
        `oceans/${model}`,
      ],
      {
        cwd: workspace,
        env: createHarnessEnvironment(isolated, {
          OCEANS_API_KEY: this.#runtime.apiKey,
          OCEANS_BASE_URL: this.#runtime.baseUrl,
          OTEL_SDK_DISABLED: "true",
          PI_CODING_AGENT_DIR: agentDir,
          // Print mode otherwise starts the turn without a server that failed to connect.
          ...(options.mcp ? { OMP_MCP_REQUIRE_READY: "1" } : {}),
        }),
        stdin: prompt,
        timeoutMs: HARNESS_TIMEOUT_MS,
      },
    );
    return {
      output: parseAssistantOutput(result.stdout),
      requestTag,
      toolCalls: parseToolCalls(result.stdout),
    };
  }
}

/**
 * An OMP extension that guards and confines `bash`, keeps `read` and `write` in the workspace, and
 * blocks every other tool except the Oceans MCP tools. OMP runs the input that a `tool_call`
 * handler returns and ignores changes to `event.input`. It checks the returned input against the
 * tool schema again, and shows it in `tool_execution_start`.
 */
export function ompSandboxSource(workspace: string): string {
  return `${sandboxHelpersSource(workspace)}
export default function workspaceSandbox(omp) {
  omp.on("tool_call", async (event) => {
    const input = event.input ?? {};
    if (event.toolName === "bash") {
      if (typeof input.command !== "string") {
        return { block: true, reason: "Shell command is missing" };
      }
      if (input.cwd !== undefined) {
        const reason = workspacePathError(input.cwd);
        if (reason) {
          return { block: true, reason };
        }
      }
      let confinedCommand;
      try {
        confinedCommand = confineShell(await guardShell(input.command));
      } catch (error) {
        return { block: true, reason: String(error) };
      }
      if (confinedCommand === null) {
        return { block: true, reason: "Shell sandboxing is unavailable on this platform" };
      }
      return { input: { ...input, command: confinedCommand } };
    }
    if (event.toolName === "read" || event.toolName === "write") {
      const reason = workspacePathError(input.path);
      return reason ? { block: true, reason } : undefined;
    }
    if (event.toolName.startsWith("mcp__oceans_")) {
      return undefined;
    }
    return { block: true, reason: "The Oceans harness allows only read, write, bash and Oceans MCP tools" };
  });
}
`;
}
