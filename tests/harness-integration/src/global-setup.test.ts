import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";

import { afterEach, describe, expect, test, vi } from "vitest";

import setup, { externalAllowlistedUser, externalRuntime, gatewayConfig, managedChatgptCredentials, normalizeExternalBaseUrl } from "./global-setup.js";
import { matrixProviders } from "./matrix.js";

describe("external gateway configuration", () => {
  afterEach(() => vi.unstubAllEnvs());

  test("rejects managed ChatGPT credentials before connecting to an external gateway", async () => {
    vi.stubEnv("GATEWAY_BASE_URL", "https://gateway.example.com");
    vi.stubEnv("OCEANS_HARNESS_CHATGPT_CREDENTIALS", "/unused/credentials.json");
    await expect(setup({} as never)).rejects.toThrow(
      /OCEANS_HARNESS_CHATGPT_CREDENTIALS.*managed-mode only.*GATEWAY_BASE_URL/,
    );
  });

  test("maps external ChatGPT and Vertex model keys independently", () => {
    vi.stubEnv("GATEWAY_ADMIN_EMAIL", "admin@example.com");
    vi.stubEnv("GATEWAY_ADMIN_PASSWORD", "password");
    vi.stubEnv("OCEANS_API_KEY", "gwk_test.secret");
    vi.stubEnv("OCEANS_ALLOWLISTED_USER_API_KEY", "");
    vi.stubEnv("OCEANS_ALLOWLISTED_TEST_MODEL", "");
    vi.stubEnv("OCEANS_CHATGPT_MODEL", "my-chatgpt-model");
    vi.stubEnv("OCEANS_VERTEX_GOOGLE_MODEL", "my-vertex-model");
    expect(externalRuntime("https://gateway.example.com").providerModels).toEqual({
      chatgpt: { openai: "my-chatgpt-model" },
      vertex: { google: "my-vertex-model" },
    });
    vi.stubEnv("OCEANS_CHATGPT_MODEL", "");
    vi.stubEnv("OCEANS_VERTEX_GOOGLE_MODEL", "");
    expect(externalRuntime("https://gateway.example.com").providerModels).toEqual({});
  });

  test("includes ChatGPT matrix cells for either opt-in", () => {
    const base = ["openrouter", "vertex", "bedrock"];
    expect(matrixProviders({})).toEqual(base);
    expect(matrixProviders({ OCEANS_HARNESS_CHATGPT_CREDENTIALS: "credential.json" })).toEqual([...base, "chatgpt"]);
    expect(matrixProviders({ OCEANS_CHATGPT_MODEL: "my-model" })).toEqual([...base, "chatgpt"]);
    expect(matrixProviders({ OCEANS_HARNESS_CHATGPT_CREDENTIALS: "", OCEANS_CHATGPT_MODEL: "" })).toEqual(base);
    expect(matrixProviders({ OCEANS_HARNESS_CHATGPT_CREDENTIALS: "file", OCEANS_CHATGPT_MODEL: "my-model" })).toEqual([...base, "chatgpt"]);
  });

  test("allows HTTPS and loopback HTTP gateway URLs", () => {
    expect(normalizeExternalBaseUrl("https://gateway.example.com/")).toBe(
      "https://gateway.example.com",
    );
    expect(normalizeExternalBaseUrl("http://127.0.0.1:8080/")).toBe(
      "http://127.0.0.1:8080",
    );
    expect(normalizeExternalBaseUrl("http://[::1]:8080/")).toBe("http://[::1]:8080");
  });

  test("rejects cleartext remote gateways and ambiguous base URLs", () => {
    expect(() => normalizeExternalBaseUrl("http://gateway.example.com")).toThrow(/HTTPS/);
    expect(() => normalizeExternalBaseUrl("https://gateway.example.com?tenant=test")).toThrow(
      /query string or fragment/,
    );
  });

  test("requires both external allowlist settings or neither", () => {
    expect(externalAllowlistedUser(undefined, undefined)).toBeUndefined();
    expect(externalAllowlistedUser("gwk_user.secret", "allowlisted-model")).toEqual({
      apiKey: "gwk_user.secret",
      model: "allowlisted-model",
    });
    expect(() => externalAllowlistedUser("gwk_user.secret", undefined)).toThrow(
      /must be configured together/,
    );
    expect(() => externalAllowlistedUser(undefined, "allowlisted-model")).toThrow(
      /must be configured together/,
    );
  });
});

describe("managed ChatGPT configuration", () => {
  test("opts in only with a readable, writable credential file and resolves its path", async () => {
    const previous = process.env.OCEANS_HARNESS_CHATGPT_CREDENTIALS;
    const previousHome = process.env.HOME;
    const dir = await mkdtemp(join(tmpdir(), "oceans-harness-config-"));
    const path = join(dir, 'credential "test".json');
    try {
      delete process.env.OCEANS_HARNESS_CHATGPT_CREDENTIALS;
      expect(await managedChatgptCredentials()).toBeUndefined();
      expect(gatewayConfig(1, "test.db")).not.toContain("chatgpt");
      process.env.OCEANS_HARNESS_CHATGPT_CREDENTIALS = path;
      await expect(managedChatgptCredentials()).rejects.toThrow(/OCEANS_HARNESS_CHATGPT_CREDENTIALS.*readable and writable/);
      await writeFile(path, "{}");
      const credentials = await managedChatgptCredentials();
      expect(credentials).toBe(path);
      process.env.OCEANS_HARNESS_CHATGPT_CREDENTIALS = relative(process.cwd(), path);
      expect(await managedChatgptCredentials()).toBe(path);
      process.env.HOME = dir;
      process.env.OCEANS_HARNESS_CHATGPT_CREDENTIALS = '~/credential "test".json';
      const expandedCredentials = await managedChatgptCredentials();
      expect(expandedCredentials).toBe(path);
      const config = gatewayConfig(1, "test.db", undefined, undefined, expandedCredentials);
      expect(config).toContain("type: chatgpt_plan");
      expect(config).toContain(`credentials_file: ${JSON.stringify(path)}`);
      expect(config).toContain("id: chatgpt/gpt-6-luna");
      expect(config).toContain("provider: chatgpt");
      expect(config).toContain("responses: true");
      expect(config).toContain("stream: true");
    } finally {
      if (previous === undefined) delete process.env.OCEANS_HARNESS_CHATGPT_CREDENTIALS;
      else process.env.OCEANS_HARNESS_CHATGPT_CREDENTIALS = previous;
      if (previousHome === undefined) delete process.env.HOME;
      else process.env.HOME = previousHome;
      await rm(dir, { force: true, recursive: true });
    }
  });
});
