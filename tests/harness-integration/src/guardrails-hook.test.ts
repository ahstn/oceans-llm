import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { afterEach, beforeEach, describe, expect, test, vi, type Mock } from "vitest";

import { guardrailPluginSource } from "./adapters/opencode.js";
import { workspaceSandboxSource } from "./adapters/pi.js";

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

type PiHook = (event: {
  toolName: string;
  input: { command: string };
}) => Promise<{ block: boolean; reason: string } | undefined>;

/** Runs a shell command through both OpenCode hooks and returns the command that would spawn. */
type OpenCodeHook = (command: string) => Promise<string>;
type OpenCodeToolEvent = { tool: string; input: Record<string, unknown> };
type OpenCodeShellEvent = { command: string };

async function loadPiHook(): Promise<PiHook> {
  const module = (await loadSource(
    workspaceSandboxSource("/tmp/guardrail-workspace"),
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

async function loadOpenCodeHooks(): Promise<{
  beforeTool: (event: OpenCodeToolEvent) => Promise<void>;
  beforeShell: (event: OpenCodeShellEvent) => void;
}> {
  const module = (await loadSource(
    guardrailPluginSource("/tmp/guardrail-workspace"),
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
