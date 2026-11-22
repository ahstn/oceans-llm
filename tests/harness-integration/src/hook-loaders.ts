import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { claudeCodeHookSource } from "./adapters/claude-code.js";
import { ompSandboxSource } from "./adapters/omp.js";
import { guardrailPluginSource } from "./adapters/opencode.js";
import { piSandboxSource } from "./adapters/pi.js";

const temporaryDirectories: string[] = [];

export type PiHook = (event: {
  toolName: string;
  input: Record<string, unknown>;
}) => Promise<{ block: boolean; reason: string } | undefined>;
export type OmpHook = (event: {
  toolName: string;
  input: Record<string, unknown>;
}) => Promise<{ block?: boolean; reason?: string; input?: Record<string, unknown> } | undefined>;
export type ClaudeCodeHook = (event: {
  tool_name: string;
  tool_input: Record<string, unknown>;
}) => Promise<{
  hookSpecificOutput: {
    hookEventName: string;
    permissionDecision: string;
    permissionDecisionReason: string;
    updatedInput?: Record<string, unknown>;
  };
}>;

/** Runs a shell command through both OpenCode hooks and returns the command that would spawn. */
export type OpenCodeHook = (command: string) => Promise<string>;
type OpenCodeToolEvent = { tool: string; input: Record<string, unknown> };
type OpenCodeShellEvent = { command: string };

/**
 * Loaders for the guardrail hook that each adapter generates, as the harness would load it from a
 * file. A hook reads `OCEANS_BASE_URL` when it loads and `OCEANS_API_KEY` on each check, and it
 * confines commands to `workspace`.
 */
export function loadHooks(workspace: string) {
  async function loadPiHook(): Promise<PiHook> {
    const module = (await loadSource(
      piSandboxSource(workspace),
      "pi",
    )) as {
      default: (pi: { on: (event: string, callback: PiHook) => void }) => void;
    };
    let hook: PiHook | undefined;
    module.default({
      on(event: string, callback: PiHook) {
        if (event === "tool_call") hook = callback;
      },
    });
    if (!hook) throw new Error("Pi guardrail hook was not registered");
    return hook;
  }

  async function loadOmpHook(): Promise<OmpHook> {
    const module = (await loadSource(ompSandboxSource(workspace), "omp")) as {
      default: (omp: { on: (event: string, callback: OmpHook) => void }) => void;
    };
    let hook: OmpHook | undefined;
    module.default({
      on(event: string, callback: OmpHook) {
        if (event === "tool_call") hook = callback;
      },
    });
    if (!hook) throw new Error("OMP guardrail hook was not registered");
    return hook;
  }

  async function loadClaudeCodeHook(): Promise<ClaudeCodeHook> {
    const module = (await loadSource(claudeCodeHookSource(workspace), "claude-code")) as {
      decidePreToolUse: ClaudeCodeHook;
    };
    return module.decidePreToolUse;
  }

  async function loadOpenCodeHooks(): Promise<{
    beforeTool: (event: OpenCodeToolEvent) => Promise<void>;
    beforeShell: (event: OpenCodeShellEvent) => void;
  }> {
    const module = (await loadSource(
      guardrailPluginSource(workspace),
      "opencode",
    )) as {
      default: { setup: (ctx: unknown) => Promise<void> };
    };
    let toolHook: ((event: OpenCodeToolEvent) => Promise<void>) | undefined;
    let shellHook: ((event: OpenCodeShellEvent) => void) | undefined;
    await module.default.setup({
      tool: {
        hook: async (name: string, callback: typeof toolHook) => {
          if (name === "execute.before") toolHook = callback;
        },
      },
      shell: {
        hook: async (name: string, callback: typeof shellHook) => {
          if (name === "create.before") shellHook = callback;
        },
      },
    });
    if (!toolHook || !shellHook) throw new Error("OpenCode guardrail hooks were not registered");
    return { beforeTool: toolHook, beforeShell: shellHook };
  }

  async function loadOpenCodeHook(): Promise<OpenCodeHook> {
    const { beforeTool, beforeShell } = await loadOpenCodeHooks();
    return async (command) => {
      const toolEvent = { tool: "shell", input: { command } as Record<string, unknown> };
      await beforeTool(toolEvent);
      const shellEvent = { command: String(toolEvent.input.command) };
      beforeShell(shellEvent);
      return shellEvent.command;
    };
  }

  return { loadClaudeCodeHook, loadOmpHook, loadOpenCodeHook, loadOpenCodeHooks, loadPiHook };
}

/** Removes the files of every hook loaded so far. */
export async function removeLoadedHooks(): Promise<void> {
  await Promise.all(
    temporaryDirectories
      .splice(0)
      .map((directory) => rm(directory, { force: true, recursive: true })),
  );
}

async function loadSource(source: string, name: string): Promise<Record<string, unknown>> {
  const directory = await mkdtemp(join(tmpdir(), `oceans-${name}-hook-`));
  temporaryDirectories.push(directory);
  const path = join(directory, `${name}.mjs`);
  await writeFile(path, source, "utf8");
  return import(`${pathToFileURL(path).href}?test=${Date.now()}-${Math.random()}`);
}
