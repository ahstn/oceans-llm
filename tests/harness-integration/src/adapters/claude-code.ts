import { randomUUID } from "node:crypto";
import { realpath, writeFile } from "node:fs/promises";
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
const CLAUDE_BINARY = join(PACKAGE_ROOT, "node_modules", ".bin", "claude");
const BUILT_IN_TOOLS = ["Read", "Write", "Edit", "Bash"];

// The "Gateway model settings" block. Other fields pass through unchanged, so Claude Code itself
// validates the full generated shape.
const GeneratedSettingsSchema = z
  .object({
    env: z
      .object({ ANTHROPIC_AUTH_TOKEN: z.string(), ANTHROPIC_BASE_URL: z.string() })
      .catchall(z.string()),
  })
  .passthrough();
export type GeneratedClaudeSettings = z.infer<typeof GeneratedSettingsSchema>;

/**
 * Parses the generated `settings.json` and removes the `ANTHROPIC_AUTH_TOKEN` placeholder. A
 * settings value overrides the process environment, and the adapter passes the real key only in
 * the process environment. Claude Code sends the key to `ANTHROPIC_BASE_URL`, which comes from the
 * gateway's client-config setting, so the URL gets the same cleartext check as `GATEWAY_BASE_URL`.
 */
export function parseGeneratedClaudeSettings(value: unknown): GeneratedClaudeSettings {
  const settings = GeneratedSettingsSchema.parse(value);
  parseCredentialUrl(settings.env.ANTHROPIC_BASE_URL, "Generated Claude Code ANTHROPIC_BASE_URL");
  return {
    ...settings,
    env: Object.fromEntries(
      Object.entries(settings.env).filter(([name]) => name !== "ANTHROPIC_AUTH_TOKEN"),
    ) as GeneratedClaudeSettings["env"],
  };
}

/**
 * Claude Code with the `settings.json` that Oceans generates. The config folder is isolated, only
 * user settings load, and a PreToolUse hook guards every tool call. The run never uses `--bare`,
 * which would skip hooks.
 */
export class ClaudeCodeAdapter implements HarnessAdapter {
  readonly harness = "claude_cli" as const;
  readonly label = "Claude Code";
  readonly tools = { read: "Read", shell: "Bash", write: ["Write"] };
  readonly #runtime: GatewayRuntime;
  readonly #generated = new Map<string, Promise<GeneratedClaudeSettings>>();

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
    let generated = this.#generated.get(model);
    if (!generated) {
      // The first block is "Gateway model settings". The optional "Lower token usage settings"
      // block turns on tool search, which the adapter does not use.
      generated = fetchGeneratedClientConfig(this.#runtime, model, "claude-code").then(
        ([content]) => parseGeneratedClaudeSettings(JSON.parse(content ?? "null")),
      );
      this.#generated.set(model, generated);
    }
    const settings = await generated;
    const isolated = await createIsolatedPaths(workspace, this.harness);
    const hookPath = join(isolated.config, "oceans-pre-tool-use.mjs");
    const mcpConfigPath = join(isolated.config, "oceans-mcp.json");
    const files = [
      // Claude Code gives tools the real path of the working folder.
      writeFile(hookPath, claudeCodeHookSource(await realpath(workspace)), "utf8"),
      writeFile(
        join(isolated.config, "settings.json"),
        JSON.stringify({
          ...settings,
          hooks: {
            // No matcher, so the hook sees every tool call. A hook that fails blocks the call.
            PreToolUse: [
              {
                hooks: [
                  {
                    type: "command",
                    command: process.execPath,
                    args: [hookPath],
                    onFailure: "block",
                  },
                ],
              },
            ],
          },
        }),
        "utf8",
      ),
    ];
    if (options.mcp) {
      files.push(
        writeFile(
          mcpConfigPath,
          JSON.stringify({
            mcpServers: {
              oceans: {
                type: "http",
                url: `${this.#runtime.baseUrl}/mcp`,
                // Claude Code reads its own credential variables as empty in MCP headers.
                headers: { Authorization: "Bearer ${OCEANS_API_KEY}" },
                // `--tools` leaves out tool search, so load the Oceans tools before the first turn.
                alwaysLoad: true,
              },
            },
          }),
          "utf8",
        ),
      );
    }
    await Promise.all(files);

    const result = await runCommand(
      CLAUDE_BINARY,
      [
        "--print",
        "--output-format",
        "stream-json",
        "--verbose",
        "--model",
        model,
        "--setting-sources",
        "user",
        "--strict-mcp-config",
        ...(options.mcp ? ["--mcp-config", mcpConfigPath] : []),
        "--tools",
        BUILT_IN_TOOLS.join(","),
        "--allowedTools",
        [...BUILT_IN_TOOLS, ...(options.mcp ? ["mcp__oceans"] : [])].join(","),
        "--permission-mode",
        "dontAsk",
        "--no-session-persistence",
        "--max-turns",
        "10",
        "--max-budget-usd",
        "1",
      ],
      {
        cwd: workspace,
        env: createHarnessEnvironment(isolated, {
          ANTHROPIC_AUTH_TOKEN: this.#runtime.apiKey,
          ANTHROPIC_CUSTOM_HEADERS: `x-oceans-tags: harness_run=${requestTag}`,
          CLAUDE_CONFIG_DIR: isolated.config,
          CLAUDE_CODE_DISABLE_AUTO_MEMORY: "1",
          CLAUDE_CODE_DISABLE_CLAUDE_MDS: "1",
          CLAUDE_CODE_DISABLE_FEATURE_FLAGS: "1",
          CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "1",
          CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL: "1",
          CLAUDE_CODE_DISABLE_TERMINAL_TITLE: "1",
          CLAUDE_CODE_SKIP_PROMPT_HISTORY: "1",
          CLAUDE_CODE_STARTUP_FAILURE_RESULTS: "1",
          DISABLE_AUTOUPDATER: "1",
          DISABLE_ERROR_REPORTING: "1",
          DISABLE_INSTALLATION_CHECKS: "1",
          DISABLE_TELEMETRY: "1",
          OCEANS_API_KEY: this.#runtime.apiKey,
          OCEANS_BASE_URL: this.#runtime.baseUrl,
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
 * A Claude Code PreToolUse command hook. It reads the hook event on stdin and prints the decision:
 * `Bash` runs the guarded command in the workspace sandbox, file tools stay in the workspace, the
 * Oceans MCP tools are allowed, and every other tool is denied. `decidePreToolUse` is exported for
 * tests.
 */
export function claudeCodeHookSource(workspace: string): string {
  return `${sandboxHelpersSource(workspace)}
import { pathToFileURL } from "node:url";

function decision(permissionDecision, permissionDecisionReason, updatedInput) {
  return {
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision,
      permissionDecisionReason,
      ...(updatedInput === undefined ? {} : { updatedInput }),
    },
  };
}

export async function decidePreToolUse(event) {
  const input = event.tool_input ?? {};
  if (event.tool_name === "Bash") {
    if (typeof input.command !== "string") {
      return decision("deny", "Shell command is missing");
    }
    let confinedCommand;
    try {
      confinedCommand = confineShell(await guardShell(input.command));
    } catch (error) {
      return decision("deny", String(error));
    }
    if (confinedCommand === null) {
      return decision("deny", "Shell sandboxing is unavailable on this platform");
    }
    // updatedInput replaces the whole tool input, so keep the other fields.
    return decision("allow", "Oceans guardrail allowed the command in the workspace sandbox", {
      ...input,
      command: confinedCommand,
    });
  }
  if (["Edit", "Read", "Write"].includes(event.tool_name)) {
    const reason = workspacePathError(input.file_path);
    return reason ? decision("deny", reason) : decision("allow", "The file is in the harness workspace");
  }
  if (typeof event.tool_name === "string" && event.tool_name.startsWith("mcp__oceans__")) {
    return decision("allow", "Oceans MCP tool");
  }
  return decision("deny", "The Oceans harness allows only Read, Write, Edit, Bash and Oceans MCP tools");
}

// An exception exits non-zero, and onFailure "block" then blocks the call.
if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  let event = "";
  for await (const chunk of process.stdin) {
    event += chunk;
  }
  process.stdout.write(JSON.stringify(await decidePreToolUse(JSON.parse(event))));
}
`;
}
