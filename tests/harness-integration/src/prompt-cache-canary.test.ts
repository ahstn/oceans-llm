import { randomUUID } from "node:crypto";

import { describe, expect, inject, test } from "vitest";

import { assertSuccessful, GatewayAdminClient } from "./gateway-client.js";

const cacheCanaryModel = process.env.OCEANS_CACHE_CANARY_MODEL;

describe.skipIf(!cacheCanaryModel)("Responses prompt-cache canary", () => {
  const runtime = inject("gateway");
  const model = cacheCanaryModel ?? runtime.model;
  const admin = new GatewayAdminClient({ ...runtime, model });

  test("records a first-turn write and a second-turn read on one route", async () => {
    await admin.login();
    const cacheKey = `oceans-cache-canary-${randomUUID()}`;
    const stablePrefix = Array.from(
      { length: 500 },
      (_, index) => `Stable cache canary sentence ${index}: preserve this exact prefix.`,
    ).join("\n");

    const first = await sendResponse(runtime.baseUrl, runtime.apiKey, model, cacheKey, stablePrefix);
    const second = await sendResponse(runtime.baseUrl, runtime.apiKey, model, cacheKey, stablePrefix);
    const firstDetail = await loggedDetail(admin, first.requestTag);
    const secondDetail = await loggedDetail(admin, second.requestTag);
    const firstUsage = cacheUsage(firstDetail.payload?.response_json);
    const secondUsage = cacheUsage(secondDetail.payload?.response_json);

    expect(firstUsage.cacheWriteTokens).toBeGreaterThan(0);
    expect(secondUsage.cacheReadTokens).toBeGreaterThan(0);

    const firstAttempt = successfulAttempt(firstDetail.attempts);
    const secondAttempt = successfulAttempt(secondDetail.attempts);
    expect(secondAttempt).toMatchObject({
      provider_key: firstAttempt.provider_key,
      route_id: firstAttempt.route_id,
      upstream_model: firstAttempt.upstream_model,
    });
    expect(firstDetail.payload?.request_json).toMatchObject({
      body: { prompt_cache_key: cacheKey, prompt_cache_retention: "in_memory" },
    });
    expect(secondDetail.payload?.request_json).toMatchObject({
      body: { prompt_cache_key: cacheKey, prompt_cache_retention: "in_memory" },
    });
  }, 180_000);
});

async function sendResponse(
  baseUrl: string,
  apiKey: string,
  model: string,
  cacheKey: string,
  stablePrefix: string,
): Promise<{ requestTag: string }> {
  const requestTag = randomUUID();
  const response = await fetch(`${baseUrl}/v1/responses`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${apiKey}`,
      "content-type": "application/json",
      "x-oceans-tags": `harness_run=${requestTag}`,
    },
    body: JSON.stringify({
      input: [{ role: "user", content: [{ type: "input_text", text: stablePrefix }] }],
      max_output_tokens: 32,
      model,
      prompt_cache_key: cacheKey,
      prompt_cache_retention: "in_memory",
    }),
    signal: AbortSignal.timeout(120_000),
  });
  await assertSuccessful(response, "prompt-cache canary response");
  await response.arrayBuffer();
  return { requestTag };
}

async function loggedDetail(admin: GatewayAdminClient, requestTag: string) {
  const log = await admin.waitForSuccessfulModelLog(requestTag);
  return admin.getRequestLogDetail(log.request_log_id);
}

function successfulAttempt(attempts: Array<{
  provider_key: string;
  route_id: string;
  status_code: number | null;
  upstream_model: string;
}>) {
  const attempt = attempts.find((candidate) => candidate.status_code === 200);
  if (!attempt) {
    throw new Error("Prompt-cache canary request log has no successful attempt");
  }
  return attempt;
}

function cacheUsage(value: unknown): { cacheReadTokens: number; cacheWriteTokens: number } {
  if (!isRecord(value)) {
    throw new Error("Prompt-cache canary response payload is not an object");
  }
  const responseBody = isRecord(value.body) ? value.body : value;
  const usage = isRecord(responseBody.usage) ? responseBody.usage : undefined;
  if (!usage) {
    throw new Error("Prompt-cache canary response payload has no usage object");
  }
  const providerUsage = isRecord(usage.provider_usage) ? usage.provider_usage : undefined;
  const details = isRecord(usage.input_tokens_details)
    ? usage.input_tokens_details
    : isRecord(providerUsage?.input_tokens_details)
      ? providerUsage.input_tokens_details
      : undefined;
  return {
    cacheReadTokens: numeric(details?.cached_tokens ?? providerUsage?.cached_tokens),
    cacheWriteTokens: numeric(details?.cache_write_tokens ?? providerUsage?.cache_write_tokens),
  };
}

function numeric(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
