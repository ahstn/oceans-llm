import { randomUUID } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { z } from "zod";

import { fetchGeneratedClientConfig, parseCredentialUrl } from "../gateway-client.js";
import { runCommand } from "../process.js";
import type { GatewayRuntime, HarnessAdapter, HarnessRun, HarnessRunOptions } from "../types.js";
import { parseAssistantOutput, parseToolCalls } from "./events.js";
import { createHarnessEnvironment, createIsolatedPaths, HARNESS_TIMEOUT_MS } from "./isolation.js";
import { sandboxHelpersSource } from "./sandbox.js";

const PACKAGE_ROOT = resolve(fileURLToPath(new URL("../..", import.meta.url)));
const PI_BINARY = join(PACKAGE_ROOT, "node_modules", ".bin", "pi");
const MCP_EXTENSION = join(PACKAGE_ROOT, "node_modules", "pi-mcp-adapter", "index.ts");

// The subset of the generated `models.json` that the adapter relies on. Other fields pass through
// unchanged, so Pi itself validates the full generated shape.
const GeneratedPiProviderSchema = z
  .object({
    api: z.string(),
    apiKey: z.string(),
    baseUrl: z.string(),
    headers: z.record(z.string(), z.string()).optional(),
    models: z.array(z.object({ id: z.string() }).passthrough()),
  })
  .passthrough();
const GeneratedPiConfigSchema = z.object({
  providers: z.record(z.string(), GeneratedPiProviderSchema),
});

export interface GeneratedPiProvider {
  id: string;
  /** The environment variable that the generated `apiKey` names. */
  keyVariable: string;
  provider: z.infer<typeof GeneratedPiProviderSchema>;
}

/**
 * Picks the provider that serves `model` from the `models.json` that Oceans generates. Pi sends
 * the API key to the provider's `baseUrl`, which comes from the gateway's client-config setting,
 * so the URL gets the same cleartext check as `GATEWAY_BASE_URL`.
 */
export function parseGeneratedPiConfig(value: unknown, model: string): GeneratedPiProvider {
  const { providers } = GeneratedPiConfigSchema.parse(value);
  const match = Object.entries(providers).find(([, provider]) =>
    provider.models.some((candidate) => candidate.id === model),
  );
  if (!match) {
    throw new Error(`Generated Pi config has no provider for model ${model}`);
  }
  const [id, provider] = match;
  parseCredentialUrl(provider.baseUrl, `Generated Pi provider ${id} baseUrl`);
  const keyVariable = /^\$([A-Za-z_][A-Za-z0-9_]*)$/.exec(provider.apiKey)?.[1];
  if (!keyVariable) {
    throw new Error(`Generated Pi provider ${id} apiKey must reference an environment variable`);
  }
  return { id, keyVariable, provider };
}

export class PiAdapter implements HarnessAdapter {
  readonly harness = "pi" as const;
  readonly label = "Pi";
  readonly tools = { read: "read", shell: "bash", write: ["write"] };
  readonly #runtime: GatewayRuntime;
  readonly #generated = new Map<string, Promise<GeneratedPiProvider>>();

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
    // Drive Pi with the `models.json` that Oceans generates for users. Only the request tag is
    // added to it.
    let generated = this.#generated.get(model);
    if (!generated) {
      generated = fetchGeneratedClientConfig(this.#runtime, model, "pi").then(([content]) =>
        parseGeneratedPiConfig(JSON.parse(content ?? "null"), model),
      );
      this.#generated.set(model, generated);
    }
    const { id, keyVariable, provider } = await generated;
    const isolated = await createIsolatedPaths(workspace, this.harness);
    const agentDir = join(isolated.config, "agent");
    await mkdir(agentDir, { recursive: true });
    const sandboxExtension = join(agentDir, "workspace-sandbox.mjs");
    const extensions = ["--extension", sandboxExtension];
    const files = [
      writeFile(
        join(agentDir, "models.json"),
        JSON.stringify({
          providers: {
            [id]: {
              ...provider,
              headers: { ...provider.headers, "x-oceans-tags": `harness_run=${requestTag}` },
            },
          },
        }),
        "utf8",
      ),
      writeFile(sandboxExtension, piSandboxSource(workspace), "utf8"),
    ];
    if (options.mcp) {
      extensions.push("--extension", MCP_EXTENSION);
      files.push(
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
      );
    }
    await Promise.all(files);

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
        ...extensions,
        "--provider",
        id,
        "--model",
        model,
        "--approve",
      ],
      {
        cwd: workspace,
        env: createHarnessEnvironment(isolated, {
          [keyVariable]: this.#runtime.apiKey,
          OCEANS_API_KEY: this.#runtime.apiKey,
          OCEANS_BASE_URL: this.#runtime.baseUrl,
          PI_OFFLINE: "1",
          PI_CODING_AGENT_DIR: agentDir,
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

/** A Pi extension that guards and confines `bash`, and keeps file tools in the workspace. */
export function piSandboxSource(workspace: string): string {
  return `${sandboxHelpersSource(workspace)}
export default function workspaceSandbox(pi) {
  pi.on("tool_call", async (event) => {
    if (event.toolName === "bash") {
      const command = event.input?.command;
      if (typeof command !== "string") {
        return { block: true, reason: "Shell command is missing" };
      }
      let confinedCommand;
      try {
        confinedCommand = confineShell(await guardShell(command));
      } catch (error) {
        return { block: true, reason: String(error) };
      }
      if (confinedCommand === null) {
        return { block: true, reason: "Shell sandboxing is unavailable on this platform" };
      }
      // Pi runs the arguments that tool_call handlers leave in event.input.
      event.input.command = confinedCommand;
      return;
    }
    if (["edit", "read", "write"].includes(event.toolName)) {
      const reason = workspacePathError(event.input?.path);
      if (reason) {
        return { block: true, reason };
      }
    }
  });
}
`;
}
