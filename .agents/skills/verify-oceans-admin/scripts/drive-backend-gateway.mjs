#!/usr/bin/env node

import fs from "node:fs/promises";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../..");
const requireFromAdminUi = createRequire(path.join(repoRoot, "crates/admin-ui/web/package.json"));
const { chromium } = requireFromAdminUi("playwright");

const baseURL = requiredEnv("OCEANS_VERIFY_BASE_URL");
const evidenceDir = requiredEnv("OCEANS_VERIFY_EVIDENCE_DIR");
const gatewayVersion = requiredEnv("OCEANS_VERIFY_GATEWAY_VERSION");
const email = requiredEnv("OCEANS_VERIFY_ADMIN_EMAIL");
const password = requiredEnv("OCEANS_VERIFY_ADMIN_PASSWORD");
const model = "deepseek-v4-flash-0731";
const upstreamModel = "deepseek/deepseek-v4-flash-0731";
const syntheticCommand = "rm -rf /tmp/oceans-verify";
const actions = [];

await fs.mkdir(evidenceDir, { recursive: true });
const browser = await chromium.launch({ headless: true });
let apiKeyId;
let rawKey;
let page;

try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
  page = await context.newPage();
  await page.goto(`${baseURL}/admin/api-keys`, { waitUntil: "domcontentloaded" });
  await waitForSignIn(page);
  await page.getByLabel("Email").fill(email);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await Promise.all([
    page.waitForURL(/\/admin\/api-keys(?:\?|$)/),
    page.getByRole("button", { name: "Sign in" }).click(),
  ]);
  await page.getByRole("heading", { name: "API keys", exact: true }).waitFor();
  await capture(page, "01-backend-api-keys");

  const keyCatalog = await adminJson(page, "/api/v1/admin/api-keys");
  const owner =
    keyCatalog.users.find((user) => user.email === "alice@platform.local") ?? keyCatalog.users[0];
  const configuredModel = keyCatalog.models.find((entry) => entry.key === model);
  assert(owner, "API key catalog has no allowed user owner");
  assert(configuredModel, `gateway model ${model} is missing from API key grants`);

  const created = await adminJson(page, "/api/v1/admin/api-keys", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      name: `Backend gateway verification ${Date.now()}`,
      owner_kind: "user",
      owner_user_id: owner.id,
      owner_team_id: null,
      owner_service_account_id: null,
      model_grant_mode: "explicit",
      model_keys: [model],
    }),
  });
  apiKeyId = created.api_key.id;
  rawKey = created.raw_key;
  assert(typeof rawKey === "string" && rawKey.startsWith("gwk_"), "temporary raw key is missing");
  actions.push({ action: "create temporary model-scoped key", result: model });

  const evaluationResponse = await fetch(`${baseURL}/api/v1/guardrails/evaluate`, {
    method: "POST",
    headers: gatewayHeaders(rawKey),
    body: JSON.stringify({ tool_name: "bash", command: syntheticCommand }),
  });
  const evaluation = await responseJson(evaluationResponse, "direct guardrail evaluation");
  assertEqual(evaluation.action, "audit", "direct evaluation action");
  assertEqual(evaluation.allowed, true, "direct evaluation allowed state");
  assertEqual(evaluation.matched_rule?.pack_id, "core.filesystem", "direct evaluation pack");
  assertEqual(
    evaluation.matched_rule?.rule_id,
    "recursive-force-remove",
    "direct evaluation rule",
  );
  assertEqual(
    evaluation.reason_code,
    "filesystem.recursive_force_remove",
    "direct evaluation reason",
  );
  actions.push({
    action: "evaluate synthetic destructive command without execution",
    result: "core.filesystem/recursive-force-remove audited",
  });

  const liveResponse = await fetch(`${baseURL}/v1/chat/completions`, {
    method: "POST",
    headers: {
      ...gatewayHeaders(rawKey),
      "x-oceans-service": "verification",
      "x-oceans-component": "backend-gateway",
      "x-oceans-env": "local",
    },
    body: JSON.stringify({
      model,
      messages: [
        {
          role: "user",
          content: `Call bash exactly once with command: ${syntheticCommand}`,
        },
      ],
      tools: [
        {
          type: "function",
          function: {
            name: "bash",
            description: "Return a shell command for review without executing it",
            parameters: {
              type: "object",
              properties: { command: { type: "string" } },
              required: ["command"],
            },
          },
        },
      ],
      tool_choice: { type: "function", function: { name: "bash" } },
      max_tokens: 32,
    }),
  });
  const requestId = liveResponse.headers.get("x-request-id");
  const liveBody = await responseJson(liveResponse, "OpenRouter Chat Completions canary");
  assert(requestId, "live response did not include x-request-id");
  const toolCalls = liveBody.choices?.[0]?.message?.tool_calls ?? [];
  assert(toolCalls.some((call) => call.function?.name === "bash"), "live response omitted bash tool call");
  actions.push({ action: "send bounded OpenRouter tool-call canary", result: "HTTP 200" });

  const decisionPage = await poll(async () => {
    const value = await adminJson(
      page,
      `/api/v1/admin/guardrails/decisions?request_id=${encodeURIComponent(requestId)}&phase=generated_tool_call`,
    );
    return value.items.some((item) => item.pack_id === "core.filesystem") ? value : null;
  }, "request-linked generated-tool decision");
  const generatedDecision = decisionPage.items.find((item) => item.pack_id === "core.filesystem");
  assertEqual(generatedDecision.rule_id, "recursive-force-remove", "generated-tool rule");
  assertEqual(generatedDecision.action, "audit", "generated-tool action");

  const requestPage = await poll(async () => {
    const value = await adminJson(
      page,
      `/api/v1/admin/observability/request-logs?request_id=${encodeURIComponent(requestId)}`,
    );
    return value.items.length === 1 ? value : null;
  }, "request-log summary");
  const summary = requestPage.items[0];
  const detail = await adminJson(
    page,
    `/api/v1/admin/observability/request-logs/${encodeURIComponent(summary.request_log_id)}`,
  );
  assertEqual(detail.log.model_key, model, "request-log model");
  assertEqual(detail.log.resolved_model_key, model, "request-log resolved model");
  assertEqual(detail.log.provider_key, "openrouter", "request-log provider");
  assertEqual(detail.log.status_code, 200, "request-log status");
  assert(
    detail.log.tool_cardinality.invoked_tool_count >= 1,
    "request log did not count the generated tool call",
  );
  const attempt = detail.attempts.find((item) => item.provider_key === "openrouter");
  assert(attempt, "request log omitted the OpenRouter provider attempt");
  assertEqual(attempt.upstream_model, upstreamModel, "provider-attempt upstream model");
  assertEqual(attempt.status, "success", "provider-attempt status");
  const usageRecorded = detail.log.total_tokens == null || detail.log.total_tokens > 0;
  assert(usageRecorded, "provider supplied a non-positive total token count");
  actions.push({
    action: "confirm request, attempt, usage, tool, and decision records",
    result: "sanitized backend evidence matched",
  });

  const proof = {
    feature: "backend-gateway",
    gatewayVersion,
    gatewayModel: model,
    provider: "openrouter",
    upstreamModel,
    requestId,
    statusCode: detail.log.status_code,
    usageRecorded: detail.log.total_tokens != null,
    invokedToolCount: detail.log.tool_cardinality.invoked_tool_count,
    guardrail: {
      phase: generatedDecision.phase,
      action: generatedDecision.action,
      packId: generatedDecision.pack_id,
      ruleId: generatedDecision.rule_id,
      reasonCode: generatedDecision.reason_code,
    },
    payloadCaptureMode: detail.log.payload_policy.capture_mode,
    actions,
    generatedAt: new Date().toISOString(),
  };
  await fs.writeFile(
    path.join(evidenceDir, "backend-gateway-canary-proof.json"),
    `${JSON.stringify(proof, null, 2)}\n`,
  );
  console.log(`backend gateway proof passed for ${model} through OpenRouter`);
  console.log(`evidence: ${evidenceDir}`);
} finally {
  try {
    if (page && apiKeyId) {
      const revokeResponse = await page.evaluate(async (id) => {
        const response = await fetch(`/api/v1/admin/api-keys/${encodeURIComponent(id)}/revoke`, {
          method: "POST",
        });
        return response.status;
      }, apiKeyId);
      assertEqual(revokeResponse, 200, "temporary API key revocation");
      if (rawKey) {
        const rejected = await fetch(`${baseURL}/v1/models`, {
          headers: { authorization: `Bearer ${rawKey}` },
        });
        assertEqual(rejected.status, 401, "revoked key authentication");
      }
    }
  } finally {
    rawKey = undefined;
    await browser.close();
  }
}

async function waitForSignIn(currentPage) {
  await currentPage.getByRole("heading", { name: "Sign in" }).waitFor();
  await currentPage.waitForFunction(() => {
    const button = Array.from(document.querySelectorAll("button")).find(
      (candidate) => candidate.textContent?.trim() === "Sign in",
    );
    return button instanceof HTMLButtonElement && !button.disabled;
  });
}

async function adminJson(currentPage, requestPath, options) {
  return currentPage.evaluate(
    async ({ pathName, init }) => {
      const response = await fetch(pathName, init);
      const body = await response.json();
      if (!response.ok) throw new Error(`${pathName} returned ${response.status}`);
      return body.data;
    },
    { pathName: requestPath, init: options },
  );
}

async function responseJson(response, label) {
  const body = await response.json();
  if (!response.ok) {
    throw new Error(`${label} returned ${response.status}: ${JSON.stringify(body)}`);
  }
  return body;
}

async function poll(operation, label) {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    const result = await operation();
    if (result) return result;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`Timed out waiting for ${label}`);
}

async function capture(currentPage, name) {
  await currentPage.screenshot({ path: path.join(evidenceDir, `${name}.png`), fullPage: true });
  await fs.writeFile(
    path.join(evidenceDir, `${name}.aria.txt`),
    `${await currentPage.locator("body").ariaSnapshot()}\n`,
  );
}

function gatewayHeaders(key) {
  return {
    authorization: `Bearer ${key}`,
    "content-type": "application/json",
  };
}

function requiredEnv(name) {
  const value = process.env[name];
  if (!value) throw new Error(`Missing required environment variable ${name}`);
  return value;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function assertEqual(actual, expected, label) {
  if (actual !== expected) {
    throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}
