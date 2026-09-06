#!/usr/bin/env node

import fs from "node:fs/promises";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { registerServer, verifyWorkbench, createAccess } from "./mcp-browser.mjs";
import { runMcpCanaries, verifyNoMcpGrant, testRevokedKey } from "./mcp-canary.mjs";

const repoRoot = process.env.OCEANS_VERIFY_REPO_ROOT ?? path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../..");
const requireFromAdminUi = createRequire(path.join(repoRoot, "crates/admin-ui/web/package.json"));
const { chromium } = requireFromAdminUi("playwright");
const { expect } = requireFromAdminUi("playwright/test");
const baseURL = requiredEnv("OCEANS_VERIFY_BASE_URL");
const gateway = new URL(baseURL);
if (!["127.0.0.1", "localhost", "[::1]"].includes(gateway.hostname) ||
    !["http:", "https:"].includes(gateway.protocol) || gateway.username || gateway.password ||
    gateway.search || gateway.hash || gateway.pathname !== "/") {
  throw new Error("MCP verification requires a local gateway origin");
}
const evidenceDir = requiredEnv("OCEANS_VERIFY_EVIDENCE_DIR");
const runId = requiredEnv("OCEANS_VERIFY_RUN_ID");
const config = JSON.parse(await fs.readFile(requiredEnv("OCEANS_VERIFY_MCP_CANDIDATES_FILE"), "utf8"));
validateConfig(config);
const actions = [];
const failures = [];
const owned = { servers: [], toolsets: [], grants: [], apiKeyId: null, apiKeyName: null, rawKey: null };
const proof = {
  feature: "mcp", runId, gatewayVersion: requiredEnv("OCEANS_VERIFY_GATEWAY_VERSION"),
  entryPoint: `${baseURL}/admin/mcp`, candidates: [], actions, failures,
  authenticationScope: "Configured gateway static header or bearer token; OAuth consent is not exercised.",
  generatedAt: new Date().toISOString(),
};
await fs.mkdir(evidenceDir, { recursive: true });
const browser = await chromium.launch({ headless: true });
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
const page = await context.newPage();
page.setDefaultTimeout(20_000);
const ui = { page, baseURL, expect, adminJson, poll, capture, actions, owned, runId, config };

try {
  await page.goto(`${baseURL}/admin/mcp`, { waitUntil: "domcontentloaded" });
  await page.getByRole("heading", { name: "Sign in", exact: true }).waitFor();
  await capture(page, "01-mcp-login");
  await page.getByLabel("Email").fill(requiredEnv("OCEANS_VERIFY_ADMIN_EMAIL"));
  await page.getByLabel("Password", { exact: true }).fill(requiredEnv("OCEANS_VERIFY_ADMIN_PASSWORD"));
  await expect(page.getByRole("button", { name: "Sign in", exact: true })).toBeEnabled();
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.getByRole("region", { name: "MCP server registry" }).waitFor();
  await capture(page, "02-mcp-registry-entry");
  actions.push({ action: "Open protected MCP registry and sign in", result: "Browser session authenticated" });

  const ready = [];
  for (const candidate of config.candidates) {
    const result = { key: candidate.key, label: candidate.label, authMode: candidate.auth_mode, required: candidate.required !== false };
    proof.candidates.push(result);
    try {
      const registered = await registerServer(ui, candidate);
      ready.push(registered);
      Object.assign(result, { discovery: "passed", serverId: registered.server.id, toolCount: registered.toolCount, selectedTool: registered.tool.upstream_name });
    } catch (error) {
      result.discovery = "failed";
      result.failure = error.name === "TimeoutError" ? "UI or discovery timed out" : "Registration or discovery contract failed";
      failures.push({ candidate: candidate.key, phase: "discovery", required: result.required, reason: result.failure });
      // Error strings and screenshots of failed provider toasts can contain upstream content.
      console.error(`MCP candidate ${candidate.key}: ${result.failure}`);
      result.failureLocation = safeLocation(error);
    }
  }
  if (ready.length === 0) throw new Error("No MCP candidate completed discovery");
  const workbench = await verifyWorkbench(ui, ready);
  proof.workbench = workbench.proof;
  const key = await createAccess(ui, workbench.primary, ready, verifyNoMcpGrant);
  proof.access = key.proof;
  const canaries = await runMcpCanaries({ page, baseURL, rawKey: owned.rawKey, apiKeyId: owned.apiKeyId, candidates: ready, adminJson, actions, capture });
  proof.canaries = canaries;
  for (const failed of canaries.failures ?? []) failures.push({ ...failed, required: config.candidates.find((candidate) => candidate.key === failed.candidate)?.required !== false });
} catch (error) {
  failures.push({ phase: "workflow", required: true, reason: error.name === "TimeoutError" ? "Browser action timed out" : "MCP verification contract failed", location: safeLocation(error) });
  console.error("MCP workflow failed; sanitized phase evidence will be retained.");
} finally {
  proof.cleanup = await cleanup();
  owned.rawKey = null;
  await browser.close();
  proof.passed = failures.every((entry) => entry.required === false) && proof.cleanup.passed;
  await fs.writeFile(path.join(evidenceDir, "mcp-proof.json"), `${JSON.stringify(proof, null, 2)}\n`);
}
console.log(`MCP verification ${proof.passed ? "passed" : "failed"}; evidence: ${evidenceDir}`);
if (!proof.passed) process.exitCode = 1;

async function cleanup() {
  const results = [];
  async function attempt(label, operation) {
    try { await operation(); results.push({ action: label, passed: true }); }
    catch { results.push({ action: label, passed: false }); }
  }
  // Use only production APIs and IDs created by this run; cleanup must survive an open dialog.
  for (const grant of owned.grants) await attempt("Revoke owned MCP grant", () => adminJson(page, "/api/v1/admin/mcp/grants", { method: "DELETE", headers: { "content-type": "application/json" }, body: JSON.stringify(grant) }));
  if (!owned.apiKeyId && owned.apiKeyName) await attempt("Resolve pending temporary key creation", async () => {
    const items = (await adminJson(page, "/api/v1/admin/api-keys")).items;
    owned.apiKeyId = items.find((item) => item.name === owned.apiKeyName)?.id ?? null;
  });
  if (owned.apiKeyId) {
    await attempt("Revoke temporary API key", () => adminJson(page, `/api/v1/admin/api-keys/${owned.apiKeyId}/revoke`, { method: "POST" }));
    if (owned.rawKey) await attempt("Confirm revoked key receives HTTP 401", () => testRevokedKey({ baseURL, rawKey: owned.rawKey, candidates: owned.servers.map((server) => ({ server })) }));
  }
  for (const set of owned.toolsets) await attempt("Disable owned tool set", async () => {
    if (!set.id) set.id = (await adminJson(page, "/api/v1/admin/mcp/toolsets?include_disabled=true")).items.find((item) => item.toolset_key === set.toolset_key)?.id;
    if (!set.id) return;
    await adminJson(page, `/api/v1/admin/mcp/toolsets/${set.id}/disable`, { method: "POST" });
    const saved = (await adminJson(page, "/api/v1/admin/mcp/toolsets?include_disabled=true")).items.find((item) => item.id === set.id);
    if (saved?.status !== "disabled") throw new Error("Tool set remains active");
  });
  for (const server of owned.servers) await attempt("Disable owned MCP server", async () => {
    if (!server.id) server.id = (await adminJson(page, "/api/v1/admin/mcp/servers?include_disabled=true")).items.find((item) => item.server_key === server.server_key)?.id;
    if (!server.id) return;
    await adminJson(page, `/api/v1/admin/mcp/servers/${server.id}/disable`, { method: "POST" });
    const saved = (await adminJson(page, "/api/v1/admin/mcp/servers?include_disabled=true")).items.find((item) => item.id === server.id);
    if (saved?.status !== "disabled") throw new Error("Server remains active");
  });
  return { passed: results.every((result) => result.passed), recordsRetainedDisabled: true, actions: results };
}

async function adminJson(currentPage, requestPath, options) {
  return currentPage.evaluate(async ({ requestPath, options }) => {
    const response = await fetch(requestPath, { ...options, signal: AbortSignal.timeout(45_000) });
    if (!response.ok) throw new Error(`Admin API returned HTTP ${response.status}`);
    const body = await response.json();
    return body.data;
  }, { requestPath, options });
}

async function capture(currentPage, name) {
  // Key creation is never captured while its one-time secret is visible.
  if (await currentPage.getByTestId("new-api-key-raw-key").count()) throw new Error("Refusing to capture a displayed API key");
  await currentPage.screenshot({ path: path.join(evidenceDir, `${name}.png`), fullPage: true });
  await fs.writeFile(path.join(evidenceDir, `${name}.aria.txt`), `${await currentPage.locator("body").ariaSnapshot()}\n`);
}

async function poll(operation, label, timeout = 45_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const result = await operation();
    if (result) return result;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`Timed out waiting for ${label}`);
}

function requiredEnv(name) {
  const value = process.env[name];
  if (!value) throw new Error(`Missing required environment variable ${name}`);
  return value;
}

function safeLocation(error) {
  return [...(error.stack ?? "").matchAll(/\b(?:drive-mcp|mcp-browser|mcp-canary)\.mjs:\d+:\d+/g)].slice(0, 3).map((match) => match[0]);
}

function validateConfig(value) {
  if (!Array.isArray(value.candidates) || value.candidates.length < 1 || value.candidates.length > 4) throw new Error("Supply one to four MCP candidates, including any negative-auth control");
  const seen = new Set();
  for (const candidate of value.candidates) {
    if (!/^[a-z0-9][a-z0-9-]{0,30}$/.test(candidate.key) || seen.has(candidate.key)) throw new Error("Candidate key must be unique and URL-safe");
    seen.add(candidate.key);
    if (typeof candidate.label !== "string" || !candidate.label.trim()) throw new Error("Candidate label is required");
    const url = new URL(candidate.server_url);
    if (url.protocol !== "https:" || url.username || url.password || url.hash) throw new Error("Candidate endpoint must be HTTPS without URL credentials");
    if ([...url.searchParams.keys()].some((key) => /key|token|secret|auth/i.test(key))) throw new Error("Do not place credentials in the MCP URL");
    if (!["none", "gateway_static_header", "gateway_bearer_token"].includes(candidate.auth_mode)) throw new Error("This proof supports public or gateway-authenticated discovery only");
    const auth = candidate.auth_config ?? {};
    if (Object.keys(auth).some((key) => !["secret_ref", "header_name"].includes(key))) throw new Error("Auth config must contain only an environment reference and optional header name");
    if (candidate.auth_mode !== "none" && !/^env\/OCEANS_MCP_DISCOVERY_[A-Z0-9_]+$/.test(auth.secret_ref ?? "")) throw new Error("Auth secret_ref must use env/OCEANS_MCP_DISCOVERY_*");
    if (candidate.expect_tool_error && candidate.auth_mode === "none") throw new Error("Negative-auth control requires an authenticated server mode");
    if (!candidate.call || typeof candidate.call.name !== "string" || !candidate.call.arguments || Array.isArray(candidate.call.arguments) || typeof candidate.call.arguments !== "object") throw new Error("Supply a reviewed read-only tool call and synthetic arguments");
  }
}
