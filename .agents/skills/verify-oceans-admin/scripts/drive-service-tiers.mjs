#!/usr/bin/env node
// Paid A/B proof for service tiers through OpenRouter: default, priority, flex, a
// route-forced fast model, a silent Messages downgrade and a streamed priority request,
// plus free negative checks and discovery. See features/service-tiers.md.

import fs from "node:fs/promises";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { createProofRecorder, safeMessage } from "./proof.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../..");
const requireFromAdminUi = createRequire(path.join(repoRoot, "crates/admin-ui/web/package.json"));
const { chromium } = requireFromAdminUi("playwright");

const baseURL = requiredEnv("OCEANS_VERIFY_BASE_URL");
const evidenceDir = requiredEnv("OCEANS_VERIFY_EVIDENCE_DIR");
requiredEnv("OCEANS_VERIFY_GATEWAY_VERSION"); // recorded in the proof envelope
const email = requiredEnv("OCEANS_VERIFY_ADMIN_EMAIL");
const password = requiredEnv("OCEANS_VERIFY_ADMIN_PASSWORD");

const tieredModel = "gpt-6-luna";
const fastModel = "gpt-6-luna-fast";
const messagesModel = "claude-haiku-5.5";
const policyModel = "deepseek-v4.1-flash";
const unsupportedModel = "claude-sonnet";
const grantedModels = [tieredModel, fastModel, messagesModel, policyModel, unsupportedModel];
const prompt = "Reply with the single word ok.";
// gpt-6-luna reasons briefly before answering; 256 leaves room without a long answer.
const maxTokens = 256;
const timeoutMs = 60_000;
// OpenRouter's per-token tier prices for gpt-6-luna are 2x (priority) and 0.5x (flex)
// of default. Allow for rounding in OpenRouter's reported cost split.
const ratioBounds = { priority: [1.8, 2.2], flex: [0.4, 0.6] };

await fs.mkdir(evidenceDir, { recursive: true });
const proof = createProofRecorder({
  evidenceDir,
  file: "service-tiers-proof.json",
  featureIds: ["service-tiers"],
});
const browser = await chromium.launch({ headless: true });
const results = {};
let apiKeyId;
let rawKey;
let page;
let details;
let failure;

try {
  page = await (await browser.newContext({ viewport: { width: 1440, height: 1000 } })).newPage();
  await signIn(page);
  ({ apiKeyId, rawKey } = await createTemporaryKey(page));
  proof.pass("temporary-key-created", { observed: grantedModels.join(",") });

  await checkDiscovery();
  await checkNegativeRequests();

  results.default = await chat({ model: tieredModel });
  assertEqual(results.default.body.service_tier, "default", "baseline served tier");
  proof.pass("default-baseline", { observed: "served default" });

  results.priority = await chat({ model: tieredModel, service_tier: "fast" });
  assertEqual(results.priority.body.service_tier, "priority", "priority served tier");
  assertTierRatio("priority", results.priority, results.default);
  proof.pass("priority-ab", { observed: `prompt rate ratio ${promptRateRatio(results.priority, results.default)}` });

  results.flex = await responses({ model: tieredModel, service_tier: "flex" });
  assertEqual(results.flex.body.service_tier, "flex", "flex served tier");
  assertTierRatio("flex", results.flex, results.default);
  proof.pass("flex-ab-responses", { observed: `prompt rate ratio ${promptRateRatio(results.flex, results.default)}` });

  results.forced = await chat({ model: fastModel, service_tier: "default" });
  assertEqual(results.forced.body.service_tier, "priority", "route-forced served tier");
  proof.pass("route-forced-fast-model", { observed: "caller default overridden by route priority" });

  results.downgrade = await messages({ model: messagesModel, speed: "fast" });
  assertEqual(results.downgrade.body.usage?.speed, "standard", "Haiku fast-mode downgrade");
  proof.pass("messages-speed-downgrade", { observed: "requested fast, served standard" });

  results.stream = await streamChat({ model: tieredModel, service_tier: "priority" });
  assertEqual(results.stream.servedTier, "priority", "streamed served tier");
  proof.pass("stream-priority", { observed: "served priority in stream" });

  const logs = await checkRequestLogs();
  await inspectRequestLog(results.priority.requestId, "service-tiers-request-log-priority");
  await inspectRequestLog(results.downgrade.requestId, "service-tiers-request-log-downgrade");
  proof.pass("request-log-ui", { observed: "tier shown on request detail" });

  await checkModelInfo();

  details = {
    models: grantedModels,
    served: Object.fromEntries(
      Object.entries(results).map(([name, result]) => [name, result.servedTier ?? result.body.service_tier ?? result.body.usage?.speed]),
    ),
    requestLogs: Object.fromEntries(
      Object.entries(logs).map(([name, log]) => [name, { id: log.request_log_id, ...tierMetadata(log) }]),
    ),
  };
} catch (error) {
  failure = error;
} finally {
  try {
    if (page && apiKeyId) await revokeTemporaryKey(page, apiKeyId, rawKey);
  } catch (error) {
    proof.add({ id: "temporary-key-revoked-and-rejected", status: "fail", note: safeMessage(error) });
    failure ??= error;
  } finally {
    rawKey = undefined;
    await browser.close();
  }
}
await proof.write({ details: details ?? {}, error: details ? undefined : failure });
if (failure) throw failure;
console.log("service tier proof passed through OpenRouter");
console.log(`evidence: ${evidenceDir}`);

async function checkDiscovery() {
  const models = await gatewayJson("GET", "/v1/models");
  const tiersOf = (id) => {
    const row = models.data.find((entry) => entry.id === id);
    assert(row, `/v1/models omits ${id}`);
    return row.metadata?.service_tiers ?? row.service_tiers ?? [];
  };
  const tiered = tiersOf(tieredModel);
  for (const tier of ["priority", "flex"]) {
    assert(tiered.includes(tier), `${tieredModel} discovery omits ${tier}: ${JSON.stringify(tiered)}`);
  }
  assert(tiersOf(fastModel).includes("priority"), `${fastModel} discovery omits priority`);
  assert(!tiersOf(policyModel).includes("priority"), `${policyModel} discovery lists a blocked tier`);
  const groups = await gatewayJson("GET", "/model_group/info");
  const group = groups.data.find((entry) => entry.model_group === tieredModel);
  assert(group?.supported_openai_params?.includes("service_tier"), "model_group/info omits service_tier");
  proof.pass("discovery-lists-tiers", { observed: `${tieredModel}: ${tiered.join(",")}` });
}

async function checkNegativeRequests() {
  const cases = [
    ["invalid-tier-400", 400, { model: tieredModel, service_tier: "turbo" }],
    ["conflicting-fields-400", 400, { model: tieredModel, service_tier: "flex", speed: "fast" }],
    ["policy-blocked-403", 403, { model: policyModel, service_tier: "priority" }, "service_tier_not_allowed"],
    ["unsupported-route-400", 400, { model: unsupportedModel, service_tier: "flex" }],
  ];
  for (const [id, status, fields, code] of cases) {
    const response = await gatewayFetch("/v1/chat/completions", chatBody(fields));
    const body = await response.json();
    assertEqual(response.status, status, `${id} status`);
    if (code) assertEqual(body.error?.code, code, `${id} error code`);
    if (id === "unsupported-route-400") {
      assert(/service tier/i.test(body.error?.message ?? ""), `${id} message does not name the tier`);
    }
    proof.pass(id, { observed: `${response.status} ${body.error?.code ?? ""}`.trim() });
  }
  // Batches run at the batch rate, so a tiered item rejects the batch before it is stored.
  const batch = await gatewayFetch(
    "/api/v1/batches",
    {
      endpoint: "chat_completions",
      model: tieredModel,
      items: [{ custom_id: "tiered", body: chatBody({ service_tier: "priority" }) }],
    },
    { authorization: `Bearer ${rawKey}`, "idempotency-key": `service-tiers-${Date.now()}` },
  );
  const batchBody = await batch.json();
  assertEqual(batch.status, 400, "batch tier status");
  assert(/cannot select a service tier/.test(batchBody.error?.message ?? ""), "batch tier message");
  proof.pass("batch-item-tier-400", { observed: `${batch.status} ${batchBody.error?.code ?? ""}`.trim() });
}

async function checkRequestLogs() {
  const expected = {
    default: [results.default, null, "default"],
    priority: [results.priority, "priority", "priority"],
    flex: [results.flex, "flex", "flex"],
    forced: [results.forced, "priority", "priority"],
    downgrade: [results.downgrade, "priority", "default"],
    stream: [results.stream, "priority", "priority"],
  };
  const logs = {};
  for (const [name, [result, requested, served]] of Object.entries(expected)) {
    const log = await poll(async () => {
      const value = await adminJson(
        page,
        `/api/v1/admin/observability/request-logs?request_id=${encodeURIComponent(result.requestId)}`,
      );
      return value.items.length === 1 ? value.items[0] : null;
    }, `${name} request log`);
    const detail = await adminJson(page, `/api/v1/admin/observability/request-logs/${log.request_log_id}`);
    const tiers = tierMetadata(detail.log);
    assertEqual(tiers.requested, requested, `${name} logged requested tier`);
    assertEqual(tiers.served, served, `${name} logged served tier`);
    assertEqual(detail.log.status_code, 200, `${name} logged status`);
    logs[name] = detail.log;
  }
  proof.pass("request-log-tiers", { observed: "requested and served tiers recorded for 6 requests" });
  return logs;
}

function tierMetadata(log) {
  return {
    requested: log.metadata?.service_tier ?? null,
    served: log.metadata?.served_service_tier ?? null,
  };
}

// Opens the request detail from the filtered list and requires the tier facts on screen.
async function inspectRequestLog(requestId, name) {
  await page.goto(
    `${baseURL}/admin/observability/request-logs?request_id=${encodeURIComponent(requestId)}`,
    { waitUntil: "domcontentloaded" },
  );
  await page.getByRole("button", { name: "Inspect" }).first().click();
  const dialog = page.getByRole("dialog");
  await dialog.getByText(/service tier/i).first().waitFor({ timeout: 15_000 });
  await dialog.getByText(/priority/i).first().waitFor({ timeout: 15_000 });
  await capture(page, name);
}

// The admin model API and the Model info dialog agree on servable tiers, the allow-list and
// the route-forced tier.
async function checkModelInfo() {
  const catalog = await adminJson(page, "/api/v1/admin/models?page=1&page_size=100&include_aliases=false");
  const byId = Object.fromEntries(catalog.items.map((model) => [model.id, model]));
  const tiered = byId[tieredModel];
  const forced = byId[fastModel];
  const policy = byId[policyModel];
  assert(tiered && forced && policy, "admin models API omits a service tier model");
  for (const tier of ["default", "flex", "priority"]) {
    assert(tiered.available_service_tiers.includes(tier), `${tieredModel} cannot serve ${tier}`);
  }
  assertEqual(JSON.stringify(forced.available_service_tiers), JSON.stringify(["priority"]), "forced model tiers");
  assertEqual(forced.routes[0]?.service_tier, "priority", "forced route tier");
  assertEqual(JSON.stringify(policy.service_tiers), JSON.stringify(["flex"]), "allow-list");
  proof.pass("admin-models-api-tiers", { observed: `${tieredModel}: ${tiered.available_service_tiers.join(",")}` });

  const overview = await openModelInfo(policyModel);
  await overview.getByText("Callers may request Flex.", { exact: true }).waitFor({ timeout: 15_000 });
  await page.keyboard.press("Escape");
  const dialog = await openModelInfo(fastModel);
  await dialog.getByText("Fast (priority)", { exact: true }).first().waitFor({ timeout: 15_000 });
  await dialog.getByRole("navigation", { name: "Model info sections" }).getByRole("button", { name: "Routing", exact: true }).click();
  await dialog.getByRole("button", { name: /^Details for .*, route 1$/ }).click();
  await dialog.getByText("Fast (priority) on every request", { exact: true }).waitFor({ timeout: 15_000 });
  await capture(page, "service-tiers-model-info-routing");
  await page.keyboard.press("Escape");
  proof.pass("model-info-ui", { observed: "allow-list and forced route tier shown" });
}

async function openModelInfo(modelId) {
  // Search and the largest page keep the row on page 1 however many models are configured.
  await page.goto(`${baseURL}/admin/models?q=${encodeURIComponent(modelId)}&page_size=100`, {
    waitUntil: "domcontentloaded",
  });
  const row = page.getByTestId(`models-desktop-cell-${modelId}`).locator("xpath=ancestor::tr");
  await row.getByRole("button", { name: "Info" }).click();
  const dialog = page.getByRole("dialog", { name: "Model info" });
  await dialog.getByText("Service tiers", { exact: true }).first().waitFor({ timeout: 15_000 });
  return dialog;
}

function assertTierRatio(tier, result, baseline) {
  const ratio = promptRateRatio(result, baseline);
  const [low, high] = ratioBounds[tier];
  assert(ratio >= low && ratio <= high, `${tier} prompt rate ratio ${ratio} outside [${low}, ${high}]`);
}

// Upstream prompt cost per prompt token, relative to the default request. OpenRouter reports
// the split in cost_details for BYOK and credit requests alike: `upstream_inference_prompt_cost`
// on Chat Completions and `upstream_inference_input_cost` on Responses.
function promptRateRatio(result, baseline) {
  const rate = (usage) =>
    Number(usage.cost_details?.upstream_inference_prompt_cost ?? usage.cost_details?.upstream_inference_input_cost) /
    Number(usage.prompt_tokens ?? usage.input_tokens);
  const value = rate(result.usage) / rate(baseline.usage);
  assert(Number.isFinite(value), "provider usage omits the prompt cost split");
  return Math.round(value * 100) / 100;
}

function chatBody(fields) {
  return { messages: [{ role: "user", content: prompt }], max_tokens: maxTokens, ...fields };
}

async function chat(fields) {
  const response = await gatewayFetch("/v1/chat/completions", chatBody(fields));
  return completed(response, `chat ${JSON.stringify(fields)}`, (body) => body.usage);
}

async function responses(fields) {
  const response = await gatewayFetch("/v1/responses", {
    input: prompt,
    max_output_tokens: maxTokens,
    store: false,
    ...fields,
  });
  return completed(response, `responses ${JSON.stringify(fields)}`, (body) => body.usage);
}

async function messages(fields) {
  const response = await gatewayFetch(
    "/v1/messages",
    { messages: [{ role: "user", content: prompt }], max_tokens: 64, ...fields },
    { "x-api-key": rawKey, "anthropic-version": "2023-06-01" },
  );
  return completed(response, `messages ${JSON.stringify(fields)}`, (body) => body.usage);
}

async function streamChat(fields) {
  const response = await gatewayFetch(
    "/v1/chat/completions",
    chatBody({ ...fields, stream: true, stream_options: { include_usage: true } }),
  );
  assertEqual(response.status, 200, "stream status");
  const requestId = response.headers.get("x-request-id");
  assert(requestId, "stream omitted x-request-id");
  let servedTier;
  let usage;
  for (const line of (await response.text()).split("\n")) {
    if (!line.startsWith("data: ") || line === "data: [DONE]") continue;
    const chunk = JSON.parse(line.slice(6));
    servedTier = chunk.service_tier ?? servedTier;
    usage = chunk.usage ?? usage;
  }
  return { requestId, body: {}, usage, servedTier };
}

async function completed(response, label, usageOf) {
  const requestId = response.headers.get("x-request-id");
  const text = await response.text();
  if (!response.ok) throw new Error(`${label} returned ${response.status}: ${text.slice(0, 300)}`);
  assert(requestId, `${label} omitted x-request-id`);
  const body = JSON.parse(text);
  return { requestId, body, usage: usageOf(body) };
}

function gatewayFetch(requestPath, body, headers = { authorization: `Bearer ${rawKey}` }) {
  return fetch(`${baseURL}${requestPath}`, {
    method: "POST",
    signal: AbortSignal.timeout(timeoutMs),
    headers: { ...headers, "content-type": "application/json", "x-oceans-component": "service-tiers" },
    body: JSON.stringify(body),
  });
}

async function gatewayJson(method, requestPath) {
  const response = await fetch(`${baseURL}${requestPath}`, {
    method,
    signal: AbortSignal.timeout(timeoutMs),
    headers: { authorization: `Bearer ${rawKey}` },
  });
  const body = await response.json();
  if (!response.ok) throw new Error(`${requestPath} returned ${response.status}`);
  return body;
}

async function signIn(currentPage) {
  await currentPage.goto(`${baseURL}/admin/api-keys`, { waitUntil: "domcontentloaded" });
  await currentPage.getByRole("heading", { name: "Sign in" }).waitFor();
  await currentPage.getByLabel("Email").fill(email);
  await currentPage.getByLabel("Password", { exact: true }).fill(password);
  await Promise.all([
    currentPage.waitForURL(/\/admin\/api-keys(?:\?|$)/),
    currentPage.getByRole("button", { name: "Sign in" }).click(),
  ]);
  await currentPage.getByRole("heading", { name: "API keys", exact: true }).waitFor();
}

async function createTemporaryKey(currentPage) {
  const catalog = await adminJson(currentPage, "/api/v1/admin/api-keys");
  const owner = catalog.users.find((user) => user.email === "alice@platform.local") ?? catalog.users[0];
  assert(owner, "API key catalog has no allowed user owner");
  for (const model of grantedModels) {
    assert(catalog.models.some((entry) => entry.key === model), `gateway model ${model} is missing`);
  }
  const created = await adminJson(currentPage, "/api/v1/admin/api-keys", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      name: `Service tier verification ${Date.now()}`,
      owner_kind: "user",
      owner_user_id: owner.id,
      owner_team_id: null,
      owner_service_account_id: null,
      model_grant_mode: "explicit",
      model_keys: grantedModels,
    }),
  });
  assert(created.raw_key?.startsWith("gwk_"), "temporary raw key is missing");
  return { apiKeyId: created.api_key.id, rawKey: created.raw_key };
}

async function revokeTemporaryKey(currentPage, id, key) {
  const status = await currentPage.evaluate(
    async ({ keyId, limitMs }) =>
      (
        await fetch(`/api/v1/admin/api-keys/${encodeURIComponent(keyId)}/revoke`, {
          method: "POST",
          signal: AbortSignal.timeout(limitMs),
        })
      ).status,
    { keyId: id, limitMs: timeoutMs },
  );
  assertEqual(status, 200, "temporary API key revocation");
  const rejected = await fetch(`${baseURL}/v1/models`, {
    headers: { authorization: `Bearer ${key}` },
    signal: AbortSignal.timeout(timeoutMs),
  });
  assertEqual(rejected.status, 401, "revoked key authentication");
  proof.pass("temporary-key-revoked-and-rejected");
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
  await currentPage.screenshot({
    path: path.join(evidenceDir, `${name}.png`),
    fullPage: true,
    mask: [currentPage.getByTestId("request-log-payloads")],
  });
  proof.artifact(`${name}.png`);
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
