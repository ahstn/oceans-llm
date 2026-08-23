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

  test("OpenCode rejects a denied command in tool.execute.before", async () => {
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
      hook({ tool: "bash" }, { args: { command: "rm -rf /tmp/opencode-guardrail" } }),
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
      openCodeHook({ tool: "bash" }, { args: { command: "rm -rf ./generated" } }),
    ).resolves.toBeUndefined();
    expect(console.error).toHaveBeenCalledWith(
      "oceans_guardrail_decision_id=decision-pi-audit",
    );
    expect(console.error).toHaveBeenCalledWith(
      "oceans_guardrail_decision_id=decision-opencode-audit",
    );
  });

  test("both hooks fail closed when policy evaluation is unavailable", async () => {
    globalThis.fetch = vi.fn().mockRejectedValue(new Error("network unavailable"));
    const piHook = await loadPiHook();
    const openCodeHook = await loadOpenCodeHook();

    await expect(
      piHook({ toolName: "bash", input: { command: "printf safe" } }),
    ).resolves.toMatchObject({ block: true, reason: expect.stringContaining("network unavailable") });
    await expect(
      openCodeHook({ tool: "bash" }, { args: { command: "printf safe" } }),
    ).rejects.toThrow("network unavailable");
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
    await expect(
      openCodeHook({ tool: "bash" }, { args: { command: "printf safe" } }),
    ).rejects.toThrow("invalid response");
  });
});

type PiHook = (event: {
  toolName: string;
  input: { command: string };
}) => Promise<{ block: boolean; reason: string } | undefined>;

type OpenCodeHook = (
  input: { tool: string },
  output: { args: { command: string } },
) => Promise<void>;

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

async function loadOpenCodeHook(): Promise<OpenCodeHook> {
  const module = (await loadSource(guardrailPluginSource(), "opencode")) as {
    OceansGuardrails: () => Promise<Record<"tool.execute.before", OpenCodeHook>>;
  };
  const hooks = await module.OceansGuardrails();
  return hooks["tool.execute.before"];
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
