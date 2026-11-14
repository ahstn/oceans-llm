import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ExpectStatic } from "vitest";

import type { GatewayAdminClient, RequestLog } from "./gateway-client.js";
import type { HarnessKey, HarnessRun, Operation } from "./types.js";

/** Runs `use` in a new, empty workspace and removes the workspace afterwards. */
export async function withWorkspace<T>(
  name: string,
  use: (workspace: string) => Promise<T>,
): Promise<T> {
  const workspace = await mkdtemp(join(tmpdir(), `oceans-${name}-`));
  try {
    return await use(workspace);
  } finally {
    await rm(workspace, { force: true, recursive: true });
  }
}

export interface ExpectedRequests {
  harness: HarnessKey;
  model: string;
  operation: Operation;
  /** The gateway provider id of the route. */
  provider: string;
}

/**
 * Checks every request log of a harness run: each request succeeded on the expected model, through
 * the expected provider, on the expected gateway API, and the gateway classified the expected harness. A run
 * that calls tools makes at least two requests, so `minimum` is how many logs to wait for.
 */
export async function expectRequestLogs(
  expect: ExpectStatic,
  admin: GatewayAdminClient,
  run: HarnessRun,
  expected: ExpectedRequests,
  minimum: number,
): Promise<RequestLog[]> {
  const logs = await admin.waitForRequestLogs(run.requestTag, minimum);
  const observed = logs.map((log) => ({
    agent_harness_key: log.agent_harness_key,
    model_key: log.model_key,
    operation: log.metadata.operation,
    provider_key: log.provider_key,
    status_code: log.status_code,
  }));
  const misclassified = logs.find((log) => log.agent_harness_key !== expected.harness);
  const userAgent = misclassified
    ? (await admin.getRequestLogDetail(misclassified.request_log_id)).user_agent_raw
    : undefined;
  expect(
    observed,
    misclassified ? `request ${misclassified.request_log_id} user_agent_raw: ${userAgent}` : "",
  ).toEqual(
    logs.map(() => ({
      agent_harness_key: expected.harness,
      model_key: expected.model,
      operation: expected.operation,
      provider_key: expected.provider,
      status_code: 200,
    })),
  );
  return logs;
}

/**
 * Prints one summary line per run. It writes to stdout directly, because Vitest can drop console
 * output from concurrent tests.
 */
export function reportRun(label: string, run: HarnessRun, logs: RequestLog[]): void {
  const tools = run.toolCalls
    .filter((toolCall) => toolCall.status === "completed")
    .map((toolCall) => toolCall.name);
  const operations = [...new Set(logs.map((log) => String(log.metadata.operation)))];
  const harnesses = [...new Set(logs.map((log) => log.agent_harness_key))];
  process.stdout.write(
    `Harness run: ${label} | operation ${operations.join(",")} | harness ${harnesses.join(",")} | requests ${logs.length} | tools ${tools.join(",") || "none"}\n`,
  );
}
