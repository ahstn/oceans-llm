import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { afterEach, beforeEach, describe, expect, test, vi, type Mock } from "vitest";

import { claudeCodeHookSource } from "./adapters/claude-code.js";
import { ompSandboxSource } from "./adapters/omp.js";
import { guardrailPluginSource } from "./adapters/opencode.js";
import { piSandboxSource } from "./adapters/pi.js";
import { runCommand } from "./process.js";

const WORKSPACE = "/tmp/guardrail-workspace";
const SANDBOX = process.platform === "darwin" ? "sandbox-exec" : "bwrap";
const originalFetch = globalThis.fetch;
let temporaryDirectories: string[] = [];

beforeEach(() => {
  process.env.OCEANS_BASE_URL = "https://gateway.example";
  process.env.OCEANS_API_KEY = "test-key";
  vi.spyOn(console, "error").mockImplementation(() => undefined);
});

afterEach(async () => {
  globalThis.fetch = originalFetch;
  vi.restoreAllMocks();
  await Promise.all(
    temporaryDirectories.splice(0).map((directory) =>
      rm(directory, { force: true, recursive: true }),
    ),
  );
});

describe("shell guardrail hooks", () => {
  test("Pi blocks a denied command before any shell implementation can run", async () => {
    const hook = await loadPiHook();
    const fetchMock = vi.fn().mockResolvedValue(
      Response.json({
        decision_id: "decision-pi",
        allowed: false,
        transformed: false,
        action: "deny",
        reason_code: "filesystem.recursive_force_remove",
      }),
    );
    globalThis.fetch = fetchMock;

    const result = await hook({
      toolName: "bash",
      input: { command: "rm -rf /tmp/pi-guardrail" },
    });

    expect(result).toMatchObject({
      block: true,
      reason: expect.stringContaining("filesystem.recursive_force_remove"),
    });
    expectGuardrailRequest(fetchMock, "rm -rf /tmp/pi-guardrail");
  });

  test("OpenCode rejects a denied command in tool execute.before", async () => {
    const hook = await loadOpenCodeHook();
    const fetchMock = vi.fn().mockResolvedValue(
      Response.json({
        decision_id: "decision-opencode",
        allowed: false,
        transformed: false,
        action: "deny",
        reason_code: "filesystem.recursive_force_remove",
      }),
    );
    globalThis.fetch = fetchMock;

    await expect(
      hook("rm -rf /tmp/opencode-guardrail"),
    ).rejects.toThrow("filesystem.recursive_force_remove");
    expectGuardrailRequest(fetchMock, "rm -rf /tmp/opencode-guardrail");
  });

  test("audit decisions allow execution and retain the decision ID", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        Response.json({
          decision_id: "decision-pi-audit",
          allowed: true,
          transformed: false,
          action: "audit",
          reason_code: "filesystem.recursive_force_remove",
        }),
      )
      .mockResolvedValueOnce(
        Response.json({
          decision_id: "decision-opencode-audit",
          allowed: true,
          transformed: false,
          action: "audit",
          reason_code: "filesystem.recursive_force_remove",
        }),
      );
    globalThis.fetch = fetchMock;

    const piHook = await loadPiHook();
    const openCodeHook = await loadOpenCodeHook();
    await expect(
      piHook({ toolName: "bash", input: { command: "rm -rf ./generated" } }),
    ).resolves.toBeUndefined();
    await expect(
      openCodeHook("rm -rf ./generated"),
    ).resolves.toContain("rm -rf ./generated");
    expect(console.error).toHaveBeenCalledWith(
      "oceans_guardrail_decision_id=decision-pi-audit",
    );
    expect(console.error).toHaveBeenCalledWith(
      "oceans_guardrail_decision_id=decision-opencode-audit",
    );
  });

  test("allowed commands execute inside an OS filesystem sandbox", async () => {
    globalThis.fetch = vi.fn().mockImplementation(() =>
      Response.json({
        decision_id: "decision-allowed",
        allowed: true,
        transformed: false,
        action: "allow",
      }),
    );
    const piHook = await loadPiHook();
    const openCodeHook = await loadOpenCodeHook();
    const piEvent = {
      toolName: "bash",
      input: { command: "cat ././../../etc/passwd" },
    };

    await expect(piHook(piEvent)).resolves.toBeUndefined();
    const openCodeCommand = await openCodeHook('cat "$PWD/../../etc/passwd"');

    const sandboxExecutable = process.platform === "darwin" ? "sandbox-exec" : "bwrap";
    expect(piEvent.input.command).toContain(sandboxExecutable);
    expect(openCodeCommand).toContain(sandboxExecutable);
    expect(piEvent.input.command).toContain("/tmp/guardrail-workspace");
    expect(openCodeCommand).toContain("/tmp/guardrail-workspace");
  });

  test("both hooks fail closed when policy evaluation is unavailable", async () => {
    globalThis.fetch = vi.fn().mockRejectedValue(new Error("network unavailable"));
    const piHook = await loadPiHook();
    const openCodeHook = await loadOpenCodeHook();

    await expect(
      piHook({ toolName: "bash", input: { command: "printf safe" } }),
    ).resolves.toMatchObject({ block: true, reason: expect.stringContaining("network unavailable") });
    await expect(openCodeHook("printf safe")).rejects.toThrow("network unavailable");
  });

  test("both hooks fail closed on malformed successful responses", async () => {
    globalThis.fetch = vi.fn().mockImplementation(() => Response.json({ allowed: true }));
    const piHook = await loadPiHook();
    const openCodeHook = await loadOpenCodeHook();

    await expect(
      piHook({ toolName: "bash", input: { command: "printf safe" } }),
    ).resolves.toMatchObject({
      block: true,
      reason: expect.stringContaining("invalid response"),
    });
    await expect(openCodeHook("printf safe")).rejects.toThrow("invalid response");
  });

  test("OpenCode rejects moving the session out of the workspace", async () => {
    const fetchMock = vi.fn();
    globalThis.fetch = fetchMock;
    const { beforeTool } = await loadOpenCodeHooks();

    // Code Mode reports namespaced names, so cover both spellings.
    for (const tool of ["session_move", "opencode_session_move"]) {
      await expect(beforeTool({ tool, input: { directory: "/" } })).rejects.toThrow(
        "moving the session",
      );
    }
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

describe("workspace confinement hooks", () => {
  test("OMP returns the confined command as new input and leaves the event unchanged", async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      Response.json({
        decision_id: "decision-omp",
        allowed: true,
        transformed: false,
        action: "allow",
      }),
    );
    globalThis.fetch = fetchMock;
    const hook = await loadOmpHook();
    const event = { toolName: "bash", input: { command: "date -u +%F", timeout: 30 } };

    const result = await hook(event);

    expect(result?.input).toMatchObject({
      command: expect.stringContaining(SANDBOX),
      timeout: 30,
    });
    expect(result?.input?.command).toContain(WORKSPACE);
    // OMP ignores changes to event.input, so the hook must not rely on them.
    expect(event.input.command).toBe("date -u +%F");
    expectGuardrailRequest(fetchMock, "date -u +%F");
  });

  test("OMP blocks a denied command and a working folder outside the workspace", async () => {
    globalThis.fetch = vi.fn().mockResolvedValue(
      Response.json({
        decision_id: "decision-omp-deny",
        allowed: false,
        transformed: false,
        action: "deny",
        reason_code: "filesystem.recursive_force_remove",
      }),
    );
    const hook = await loadOmpHook();

    await expect(hook({ toolName: "bash", input: { command: "rm -rf /" } })).resolves.toMatchObject(
      { block: true, reason: expect.stringContaining("filesystem.recursive_force_remove") },
    );
    for (const cwd of ["/", "../outside", "local://", "~"]) {
      await expect(
        hook({ toolName: "bash", input: { command: "pwd", cwd } }),
        cwd,
      ).resolves.toMatchObject({
        block: true,
        reason: expect.stringContaining("outside the harness workspace"),
      });
    }
  });

  test("file tools stay in the workspace", async () => {
    const fetchMock = vi.fn();
    globalThis.fetch = fetchMock;
    const piHook = await loadPiHook();
    const ompHook = await loadOmpHook();
    const claudeHook = await loadClaudeCodeHook();

    // Pi and OMP strip a leading "@", OMP resolves internal URLs, and it accepts a hashline
    // header copied from a read in place of a path.
    for (const path of [
      "/etc/passwd",
      "../outside.txt",
      "~/.ssh/id_ed25519",
      "@/etc/passwd",
      "xd://bash",
      "local://notes.md",
      "agent://0",
      "proc://bg_1",
      "[/etc/passwd#ab12]",
      "[../outside.txt#AB12]",
      "",
    ]) {
      await expect(piHook({ toolName: "read", input: { path } }), path).resolves.toMatchObject({
        block: true,
      });
      await expect(
        ompHook({ toolName: "write", input: { path, content: "x" } }),
        path,
      ).resolves.toMatchObject({ block: true });
      await expect(
        claudeHook({ tool_name: "Read", tool_input: { file_path: path } }),
        path,
      ).resolves.toMatchObject({ hookSpecificOutput: { permissionDecision: "deny" } });
    }
    for (const path of ["source.txt", `${WORKSPACE}/nested/destination.txt`]) {
      await expect(piHook({ toolName: "write", input: { path } }), path).resolves.toBeUndefined();
      await expect(ompHook({ toolName: "read", input: { path } }), path).resolves.toBeUndefined();
      await expect(
        claudeHook({ tool_name: "Write", tool_input: { file_path: path, content: "x" } }),
        path,
      ).resolves.toMatchObject({ hookSpecificOutput: { permissionDecision: "allow" } });
    }
    expect(fetchMock).not.toHaveBeenCalled();
  });

  test("OMP and Claude Code allow only file, shell and Oceans MCP tools", async () => {
    globalThis.fetch = vi.fn();
    const ompHook = await loadOmpHook();
    const claudeHook = await loadClaudeCodeHook();

    await expect(
      ompHook({ toolName: "mcp__oceans_call_tool", input: { address: "mcp://context7/tools/docs" } }),
    ).resolves.toBeUndefined();
    for (const toolName of ["edit", "task", "browser", "mcp__other_call_tool"]) {
      await expect(ompHook({ toolName, input: {} }), toolName).resolves.toMatchObject({
        block: true,
      });
    }
    await expect(
      claudeHook({ tool_name: "mcp__oceans__search_tools", tool_input: { query: "context7" } }),
    ).resolves.toMatchObject({ hookSpecificOutput: { permissionDecision: "allow" } });
    for (const toolName of ["WebFetch", "Agent", "Monitor", "mcp__other__call_tool"]) {
      await expect(
        claudeHook({ tool_name: toolName, tool_input: {} }),
        toolName,
      ).resolves.toMatchObject({ hookSpecificOutput: { permissionDecision: "deny" } });
    }
  });

  test("Claude Code runs an allowed command in the sandbox and keeps the other Bash inputs", async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      Response.json({
        decision_id: "decision-claude",
        allowed: true,
        transformed: false,
        action: "allow",
      }),
    );
    globalThis.fetch = fetchMock;
    const hook = await loadClaudeCodeHook();

    const { hookSpecificOutput } = await hook({
      tool_name: "Bash",
      tool_input: { command: "date -u +%F", description: "Print the date", timeout: 30_000 },
    });

    // updatedInput replaces the whole tool input.
    expect(hookSpecificOutput).toMatchObject({
      hookEventName: "PreToolUse",
      permissionDecision: "allow",
      updatedInput: {
        command: expect.stringContaining(SANDBOX),
        description: "Print the date",
        timeout: 30_000,
      },
    });
    expect(hookSpecificOutput.updatedInput?.command).toContain(WORKSPACE);
    expectGuardrailRequest(fetchMock, "date -u +%F");
  });

  test("Claude Code denies a command that the guardrail denies or cannot evaluate", async () => {
    const hook = await loadClaudeCodeHook();
    globalThis.fetch = vi.fn().mockResolvedValue(
      Response.json({
        decision_id: "decision-claude-deny",
        allowed: false,
        transformed: false,
        action: "deny",
        reason_code: "filesystem.recursive_force_remove",
      }),
    );
    await expect(
      hook({ tool_name: "Bash", tool_input: { command: "rm -rf /" } }),
    ).resolves.toMatchObject({
      hookSpecificOutput: {
        permissionDecision: "deny",
        permissionDecisionReason: expect.stringContaining("filesystem.recursive_force_remove"),
      },
    });

    globalThis.fetch = vi.fn().mockRejectedValue(new Error("network unavailable"));
    await expect(
      hook({ tool_name: "Bash", tool_input: { command: "printf safe" } }),
    ).resolves.toMatchObject({
      hookSpecificOutput: {
        permissionDecision: "deny",
        permissionDecisionReason: expect.stringContaining("network unavailable"),
      },
    });
  });

  test("the Claude Code hook answers on stdout and fails on unreadable input", async () => {
    const directory = await mkdtemp(join(tmpdir(), "oceans-claude-code-hook-"));
    temporaryDirectories.push(directory);
    const script = join(directory, "hook.mjs");
    await writeFile(script, claudeCodeHookSource(WORKSPACE), "utf8");
    const env = { OCEANS_BASE_URL: "https://gateway.example", PATH: process.env.PATH };

    const { stdout } = await runCommand(process.execPath, [script], {
      cwd: directory,
      env,
      stdin: JSON.stringify({
        hook_event_name: "PreToolUse",
        tool_name: "Read",
        tool_input: { file_path: "/etc/passwd" },
      }),
    });

    expect(JSON.parse(stdout)).toMatchObject({
      hookSpecificOutput: { hookEventName: "PreToolUse", permissionDecision: "deny" },
    });
    // The adapter sets onFailure to "block", so a hook that exits with an error blocks the call.
    await expect(
      runCommand(process.execPath, [script], { cwd: directory, env, stdin: "not json" }),
    ).rejects.toThrow();
  });
});

type PiHook = (event: {
  toolName: string;
  input: Record<string, unknown>;
}) => Promise<{ block: boolean; reason: string } | undefined>;
type OmpHook = (event: {
  toolName: string;
  input: Record<string, unknown>;
}) => Promise<{ block?: boolean; reason?: string; input?: Record<string, unknown> } | undefined>;
type ClaudeCodeHook = (event: {
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
type OpenCodeHook = (command: string) => Promise<string>;
type OpenCodeToolEvent = { tool: string; input: Record<string, unknown> };
type OpenCodeShellEvent = { command: string };

async function loadPiHook(): Promise<PiHook> {
  const module = (await loadSource(
    piSandboxSource(WORKSPACE),
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
  const module = (await loadSource(ompSandboxSource(WORKSPACE), "omp")) as {
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
  const module = (await loadSource(claudeCodeHookSource(WORKSPACE), "claude-code")) as {
    decidePreToolUse: ClaudeCodeHook;
  };
  return module.decidePreToolUse;
}

async function loadOpenCodeHooks(): Promise<{
  beforeTool: (event: OpenCodeToolEvent) => Promise<void>;
  beforeShell: (event: OpenCodeShellEvent) => void;
}> {
  const module = (await loadSource(
    guardrailPluginSource(WORKSPACE),
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

async function loadSource(source: string, name: string): Promise<Record<string, unknown>> {
  const directory = await mkdtemp(join(tmpdir(), `oceans-${name}-hook-`));
  temporaryDirectories.push(directory);
  const path = join(directory, `${name}.mjs`);
  await writeFile(path, source, "utf8");
  return import(`${pathToFileURL(path).href}?test=${Date.now()}-${Math.random()}`);
}

function expectGuardrailRequest(fetchMock: Mock, command: string): void {
  expect(fetchMock).toHaveBeenCalledOnce();
  const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
  expect(url).toBe("https://gateway.example/api/v1/guardrails/evaluate");
  expect(init.method).toBe("POST");
  expect(init.signal).toBeInstanceOf(AbortSignal);
  expect(init.headers).toMatchObject({ authorization: "Bearer test-key" });
  expect(JSON.parse(String(init.body))).toEqual({ tool_name: "bash", command });
}
