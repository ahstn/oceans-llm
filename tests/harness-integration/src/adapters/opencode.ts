import { spawn } from "node:child_process";
import { randomBytes, randomUUID } from "node:crypto";
import { once } from "node:events";
import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

import { z } from "zod";

import { fetchGeneratedClientConfig, parseCredentialUrl } from "../gateway-client.js";
import { delay, runCommand } from "../process.js";
import type {
  GatewayRuntime,
  HarnessAdapter,
  HarnessRun,
  HarnessRunOptions,
} from "../types.js";
import { parseAssistantOutput, parseToolCalls } from "./events.js";
import {
  createHarnessEnvironment,
  createIsolatedPaths,
  HARNESS_TIMEOUT_MS,
  type IsolatedPaths,
} from "./isolation.js";
import { sandboxHelpersSource } from "./sandbox.js";

const PACKAGE_ROOT = resolve(fileURLToPath(new URL("../..", import.meta.url)));
const OPENCODE_BINARY = join(PACKAGE_ROOT, "node_modules", ".bin", "opencode");
const MCP_READY_TIMEOUT_MS = 60_000;

// The subset of the generated native V2 config that the adapter relies on. Other fields pass
// through unchanged so OpenCode itself validates the full generated shape.
const GeneratedConfigSchema = z
  .object({
    model: z.string(),
    providers: z.record(
      z.string(),
      z
        .object({
          env: z.array(z.string()).min(1),
          headers: z.record(z.string(), z.string()).optional(),
          models: z.record(z.string(), z.unknown()),
          package: z.string(),
          settings: z.object({ baseURL: z.string() }).passthrough(),
        })
        .passthrough(),
    ),
  })
  .passthrough();
export type GeneratedOpenCodeConfig = z.infer<typeof GeneratedConfigSchema>;

/**
 * Parses the generated config. The adapter gives each provider the Oceans API key, and OpenCode
 * sends that key to the provider's `settings.baseURL`. That URL comes from the gateway's
 * client-config setting, not from `GATEWAY_BASE_URL`, so it gets the same cleartext check.
 */
export function parseGeneratedOpenCodeConfig(value: unknown): GeneratedOpenCodeConfig {
  const config = GeneratedConfigSchema.parse(value);
  for (const [id, provider] of Object.entries(config.providers)) {
    parseCredentialUrl(provider.settings.baseURL, `Generated OpenCode provider ${id} baseURL`);
  }
  return config;
}

export async function fetchGeneratedOpenCodeConfig(
  runtime: GatewayRuntime,
  model: string,
): Promise<GeneratedOpenCodeConfig> {
  const [content] = await fetchGeneratedClientConfig(runtime, model, "opencode");
  return parseGeneratedOpenCodeConfig(JSON.parse(content ?? "null"));
}

/** OpenCode with the native config that Oceans generates for a model. */
export class OpenCodeAdapter implements HarnessAdapter {
  readonly harness = "opencode" as const;
  readonly label = "OpenCode";
  readonly tools = { read: "read", shell: "shell", write: ["write", "patch"] };
  readonly #runtime: GatewayRuntime;
  readonly #generated = new Map<string, Promise<GeneratedOpenCodeConfig>>();

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
    // Drive OpenCode with the config Oceans generates for users, so the suite proves that config
    // works in the real client. Only harness-specific settings are layered on top.
    let generated = this.#generated.get(model);
    if (!generated) {
      generated = fetchGeneratedOpenCodeConfig(this.#runtime, model);
      this.#generated.set(model, generated);
    }
    const config = await generated;
    const isolated = await createIsolatedPaths(workspace, this.harness);
    await installGuardrailPlugin(isolated, workspace);
    const providers = Object.fromEntries(
      Object.entries(config.providers).map(([id, provider]) => [
        id,
        {
          ...provider,
          headers: { ...provider.headers, "x-oceans-tags": `harness_run=${requestTag}` },
        },
      ]),
    );
    // The generated config names the API key variable in `providers.*.env`; supply the key there.
    const providerKeys = Object.fromEntries(
      Object.values(config.providers).flatMap((provider) =>
        provider.env.map((name) => [name, this.#runtime.apiKey]),
      ),
    );
    const environment = openCodeEnvironment(isolated, {
      ...providerKeys,
      OCEANS_API_KEY: this.#runtime.apiKey,
      OCEANS_BASE_URL: this.#runtime.baseUrl,
      OPENCODE_CONFIG_CONTENT: JSON.stringify({
        ...config,
        ...harnessOpenCodeConfig(this.#runtime, options),
        providers,
      }),
    });
    return {
      ...(await runOpenCode(workspace, environment, config.model, prompt, options)),
      requestTag,
    };
  }
}

/**
 * The isolated environment for an OpenCode server. It turns off update checks, Claude Code config
 * import, the file watcher and remote model lists.
 */
export function openCodeEnvironment(
  isolated: IsolatedPaths,
  overrides: NodeJS.ProcessEnv,
): NodeJS.ProcessEnv {
  return createHarnessEnvironment(isolated, {
    XDG_STATE_HOME: join(isolated.home, "state"),
    OPENCODE_CONFIG_DIR: isolated.config,
    OPENCODE_DISABLE_AUTOUPDATE: "true",
    OPENCODE_DISABLE_CLAUDE_CODE: "true",
    OPENCODE_DISABLE_FILEWATCHER: "true",
    OPENCODE_DISABLE_FREE_MODELS: "true",
    OPENCODE_DISABLE_MODELS_FETCH: "true",
    ...overrides,
  });
}

/** OpenCode settings for a harness run: a step limit, permissions, and Oceans MCP when asked. */
export function harnessOpenCodeConfig(
  runtime: GatewayRuntime,
  options: HarnessRunOptions,
): Record<string, unknown> {
  return {
    agents: {
      build: {
        steps: 8,
      },
    },
    ...(options.mcp
      ? {
          mcp: {
            servers: {
              oceans: {
                // Code Mode wraps MCP calls in generated `execute` scripts. Direct tool calls keep
                // the aggregate operations and their inputs visible in the event stream.
                codemode: false,
                headers: { Authorization: "Bearer {env:OCEANS_API_KEY}" },
                oauth: false,
                timeout: { catalog: 60_000, execution: 60_000, startup: 60_000 },
                type: "remote",
                url: `${runtime.baseUrl}/mcp`,
              },
            },
          },
        }
      : {}),
    permissions: [
      { action: "shell", effect: "allow", resource: "*" },
      { action: "external_directory", effect: "deny", resource: "*" },
      { action: "subagent", effect: "deny", resource: "*" },
    ],
  };
}

/** Adds the guardrail plugin to the isolated OpenCode config folder. */
export async function installGuardrailPlugin(
  isolated: IsolatedPaths,
  workspace: string,
): Promise<void> {
  const pluginDirectory = join(isolated.config, "plugins");
  await mkdir(pluginDirectory, { recursive: true });
  await writeFile(
    join(pluginDirectory, "oceans-guardrails.js"),
    guardrailPluginSource(workspace),
    "utf8",
  );
}

/** Runs one prompt on a private OpenCode server and parses its JSON event stream. */
export async function runOpenCode(
  workspace: string,
  environment: NodeJS.ProcessEnv,
  model: string,
  prompt: string,
  options: HarnessRunOptions,
): Promise<Omit<HarnessRun, "requestTag">> {
  const result = await withPrivateServer(workspace, environment, async (server) => {
    if (options.mcp) {
      // V2 connects MCP servers in the background after the server starts. Wait until Oceans
      // MCP is connected on this server, so the first model request already lists its tools.
      await waitForOceansMcp(workspace, server);
    }
    return runCommand(
      OPENCODE_BINARY,
      [
        "run",
        "--server",
        server.url,
        "--format",
        "json",
        "--model",
        model,
        // A session without a title asks the model for one, and the server stops when the run
        // ends, which cancels that request.
        "--title",
        "Oceans harness run",
        "--auto",
        "--",
        prompt,
      ],
      { cwd: workspace, env: server.env, timeoutMs: HARNESS_TIMEOUT_MS },
    );
  });
  return {
    output: parseAssistantOutput(result.stdout),
    toolCalls: parseToolCalls(result.stdout),
  };
}

export interface PrivateServer {
  url: string;
  /** Client environment, including the server password. */
  env: NodeJS.ProcessEnv;
}

/**
 * Starts a private OpenCode server with the isolated environment, as `--standalone` does, but keeps
 * it running across several client commands. Without a private server, OpenCode V2 reuses a shared
 * background service that ignores this process's environment and config.
 */
export async function withPrivateServer<T>(
  workspace: string,
  environment: NodeJS.ProcessEnv,
  use: (server: PrivateServer) => Promise<T>,
): Promise<T> {
  const env = { ...environment, OPENCODE_PASSWORD: randomBytes(32).toString("base64url") };
  // The server ends its lease when stdin closes, so it cannot outlive this process.
  const child = spawn(OPENCODE_BINARY, ["serve", "--stdio", "--port", "0"], {
    cwd: workspace,
    env,
    stdio: ["pipe", "pipe", "pipe"],
  });
  const stderr: string[] = [];
  child.stderr.on("data", (chunk: Buffer) => stderr.push(chunk.toString()));
  const exited = once(child, "close").then(() => undefined);
  try {
    const lines = createInterface({ input: child.stdout });
    const readyLine = await Promise.race([
      once(lines, "line").then(([line]) => String(line)),
      exited,
      delay(30_000).then(() => undefined),
    ]);
    if (readyLine === undefined) {
      throw new Error(`OpenCode server did not report readiness:\n${stderr.join("")}`);
    }
    const { url } = z.object({ url: z.string() }).parse(JSON.parse(readyLine));
    return await use({ url, env });
  } finally {
    child.stdin.end();
    child.kill("SIGTERM");
    await Promise.race([exited, delay(5_000)]);
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
  }
}

async function waitForOceansMcp(workspace: string, server: PrivateServer): Promise<void> {
  const deadline = Date.now() + MCP_READY_TIMEOUT_MS;
  let status = "";
  while (Date.now() < deadline) {
    const result = await runCommand(OPENCODE_BINARY, ["mcp", "list", "--server", server.url], {
      cwd: workspace,
      env: server.env,
      timeoutMs: 15_000,
    });
    status = result.stdout.split("\n").find((line) => /\boceans\b/.test(line)) ?? result.stdout;
    if (/\boceans\s+connected\b/.test(status)) return;
    if (/\boceans\s+(failed|needs authentication)/.test(status)) break;
    await delay(250);
  }
  throw new Error(`OpenCode did not connect to Oceans MCP: ${status.trim()}`);
}

export function guardrailPluginSource(workspace: string): string {
  return `${sandboxHelpersSource(workspace)}
async function guardToolCall(event) {
  // session_move relocates the session, which would move the workspace boundary that the
  // external_directory rule enforces. It has no permission check, so reject it here. Code Mode
  // calls also pass through this hook.
  if (event.tool.endsWith("session_move")) {
    throw new Error("Oceans harness denies moving the session out of its workspace");
  }
  if (event.tool !== "shell") return;
  const command = event.input?.command;
  if (typeof command !== "string") {
    throw new Error("Shell command is missing");
  }
  event.input.command = await guardShell(command);
}

// Confine at process creation, after OpenCode has checked permissions against the model's command.
// Rewriting the tool input instead would make OpenCode treat the sandbox paths as external directories.
function confineShellProcess(invocation) {
  const confinedCommand = confineShell(invocation.command);
  if (confinedCommand === null) {
    throw new Error("Shell sandboxing is unavailable on this platform");
  }
  invocation.command = confinedCommand;
}

export default {
  id: "oceans.guardrails",
  async setup(ctx) {
    await ctx.tool.hook("execute.before", guardToolCall);
    await ctx.shell.hook("create.before", confineShellProcess);
  },
};
`;
}
