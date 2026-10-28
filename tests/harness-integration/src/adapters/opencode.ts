import { spawn } from "node:child_process";
import { randomBytes, randomUUID } from "node:crypto";
import { once } from "node:events";
import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

import { z } from "zod";

import { fetchGeneratedOpenCodeConfig, parseCredentialUrl } from "../gateway-client.js";
import { delay, runCommand } from "../process.js";
import type { GatewayRuntime, HarnessAdapter, HarnessRun } from "../types.js";
import { parseAssistantOutput, parseToolCalls } from "./events.js";
import { createHarnessEnvironment, createIsolatedPaths } from "./isolation.js";

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
          settings: z.object({ baseURL: z.string() }).passthrough(),
        })
        .passthrough(),
    ),
  })
  .passthrough();
type GeneratedConfig = z.infer<typeof GeneratedConfigSchema>;

/**
 * Parses the generated config. The adapter gives each provider the Oceans API key, and OpenCode
 * sends that key to the provider's `settings.baseURL`. That URL comes from the gateway's
 * client-config setting, not from `GATEWAY_BASE_URL`, so it gets the same cleartext check.
 */
export function parseGeneratedOpenCodeConfig(value: unknown): GeneratedConfig {
  const config = GeneratedConfigSchema.parse(value);
  for (const [id, provider] of Object.entries(config.providers)) {
    parseCredentialUrl(provider.settings.baseURL, `Generated OpenCode provider ${id} baseURL`);
  }
  return config;
}

export class OpenCodeAdapter implements HarnessAdapter {
  readonly key = "opencode" as const;
  readonly label = "OpenCode";
  readonly #runtime: GatewayRuntime;
  #generated: Promise<GeneratedConfig> | undefined;

  constructor(runtime: GatewayRuntime) {
    this.#runtime = runtime;
  }

  async run(workspace: string, prompt: string): Promise<HarnessRun> {
    const requestTag = randomUUID();
    // Drive OpenCode with the config Oceans generates for users, so the suite proves that config
    // works in the real client. Only harness-specific settings are layered on top.
    this.#generated ??= fetchGeneratedOpenCodeConfig(this.#runtime, this.#runtime.model).then(
      parseGeneratedOpenCodeConfig,
    );
    const generated = await this.#generated;
    const isolated = await createIsolatedPaths(workspace, this.key);
    const pluginDirectory = join(isolated.config, "plugins");
    await mkdir(pluginDirectory, { recursive: true });
    await writeFile(
      join(pluginDirectory, "oceans-guardrails.js"),
      guardrailPluginSource(workspace),
      "utf8",
    );
    const providers = Object.fromEntries(
      Object.entries(generated.providers).map(([id, provider]) => [
        id,
        {
          ...provider,
          headers: { ...provider.headers, "x-oceans-tags": `harness_run=${requestTag}` },
        },
      ]),
    );
    const config = {
      ...generated,
      providers,
      agents: {
        build: {
          steps: 8,
        },
      },
      mcp: {
        servers: {
          oceans: {
            // Code Mode wraps MCP calls in generated `execute` scripts. Direct tool calls keep the
            // aggregate operations and their inputs visible in the event stream.
            codemode: false,
            headers: { Authorization: "Bearer {env:OCEANS_API_KEY}" },
            oauth: false,
            timeout: { catalog: 60_000, execution: 60_000, startup: 60_000 },
            type: "remote",
            url: `${this.#runtime.baseUrl}/mcp`,
          },
        },
      },
      permissions: [
        { action: "shell", effect: "allow", resource: "*" },
        { action: "external_directory", effect: "deny", resource: "*" },
        { action: "subagent", effect: "deny", resource: "*" },
      ],
    };
    // The generated config names the API key variable in `providers.*.env`; supply the key there.
    const providerKeys = Object.fromEntries(
      Object.values(generated.providers).flatMap((provider) =>
        provider.env.map((name) => [name, this.#runtime.apiKey]),
      ),
    );
    const environment = createHarnessEnvironment(isolated, {
      ...providerKeys,
      OCEANS_API_KEY: this.#runtime.apiKey,
      OCEANS_BASE_URL: this.#runtime.baseUrl,
      OPENCODE_CONFIG_CONTENT: JSON.stringify(config),
      OPENCODE_CONFIG_DIR: isolated.config,
      OPENCODE_DISABLE_AUTOUPDATE: "true",
      OPENCODE_DISABLE_CLAUDE_CODE: "true",
      OPENCODE_DISABLE_FILEWATCHER: "true",
      OPENCODE_DISABLE_FREE_MODELS: "true",
      OPENCODE_DISABLE_MODELS_FETCH: "true",
    });
    const result = await withPrivateServer(workspace, environment, async (server) => {
      // V2 connects MCP servers in the background after the server starts. Wait until Oceans MCP
      // is connected on this server, so the first model request already lists its tools.
      await waitForOceansMcp(workspace, server);
      return runCommand(
        OPENCODE_BINARY,
        [
          "run",
          "--server",
          server.url,
          "--format",
          "json",
          "--model",
          generated.model,
          "--auto",
          "--",
          prompt,
        ],
        { cwd: workspace, env: server.env, timeoutMs: 90_000 },
      );
    });
    return {
      output: parseAssistantOutput(result.stdout),
      requestTag,
      toolCalls: parseToolCalls(result.stdout),
    };
  }
}

interface PrivateServer {
  url: string;
  /** Client environment, including the server password. */
  env: NodeJS.ProcessEnv;
}

/**
 * Starts a private OpenCode server with the isolated environment, as `--standalone` does, but keeps
 * it running across several client commands. Without a private server, OpenCode V2 reuses a shared
 * background service that ignores this process's environment and config.
 */
async function withPrivateServer<T>(
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
  return `
const guardrailUrl = new URL("/api/v1/guardrails/evaluate", process.env.OCEANS_BASE_URL).toString();
const guardrailTimeoutMs = Number(process.env.OCEANS_GUARDRAIL_TIMEOUT_MS ?? "2000");
const workspace = ${JSON.stringify(workspace)};

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


function shellQuote(value) {
  return "'" + value.replaceAll("'", "'\\"'\\"'") + "'";
}

function confineShell(command) {
  if (process.platform === "linux") {
    return "bwrap --die-with-parent --unshare-all --new-session --tmpfs / --proc /proc --dev /dev --tmpfs /tmp --ro-bind-try /usr /usr --ro-bind-try /bin /bin --ro-bind-try /lib /lib --ro-bind-try /lib64 /lib64 --bind " +
      shellQuote(workspace) + " /workspace --chdir /workspace /bin/sh -c " + shellQuote(command);
  }
  if (process.platform === "darwin") {
    const profile = '(version 1)(deny default)(allow process*)(allow sysctl-read)(allow mach-lookup)(allow file-read* (subpath "/System") (subpath "/usr") (subpath "/bin") (subpath "/dev") (subpath ' +
      JSON.stringify(workspace) + '))(allow file-write* (subpath ' + JSON.stringify(workspace) + '))';
    return "/usr/bin/sandbox-exec -p " + shellQuote(profile) + " /bin/sh -c " + shellQuote(command);
  }
  return null;
}

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
  const decision = validateGuardrailDecision(await response.json());
  console.error("oceans_guardrail_decision_id=" + decision.decision_id);
  if (!decision.allowed) {
    throw new Error("Oceans guardrail denied shell execution: " + decision.reason_code);
  }
  if (decision.transformed) {
    if (typeof decision.output_command !== "string") {
      throw new Error("Oceans guardrail returned an invalid shell transformation");
    }
    event.input.command = decision.output_command;
  }
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
