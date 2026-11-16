import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { z } from "zod";
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
  /** The gateway provider id of the route, when known. */
  provider?: string;
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
      provider_key: expected.provider ?? expect.stringMatching(/\S/),
      status_code: 200,
    })),
  );
  return logs;
}

/** The setting that an `effort` or `fast` run asks for. */
export type RequestedSetting = { effort: string; operation: Operation } | { tier: "priority" };

/** Where the stored request body of each gateway API holds the reasoning effort. */
const EFFORT_FIELDS: Record<Operation, readonly string[]> = {
  responses: ["reasoning", "effort"],
  // The gateway stores Messages requests in its Chat representation, but keeps `output_config`.
  messages: ["output_config", "effort"],
  chat_completions: ["reasoning_effort"],
};

// Body fields that show how a request asked for reasoning, without the conversation.
const LEFT_OUT_OF_EVIDENCE: Record<string, true> = {
  input: true,
  instructions: true,
  messages: true,
  system: true,
  tools: true,
};

const StoredRequestSchema = z.object({ body: z.record(z.string(), z.unknown()) });

/**
 * Checks that every request of a run asked for the run's setting, and returns what the request
 * logs record, for the run summary. A `fast` run must record the requested `priority` tier. The
 * served tier is only reported, because OpenRouter does not always report it. An `effort` run
 * must record no tier, and its stored request body must hold the effort in the field of the
 * gateway API, as `EFFORT_FIELDS` lists. The request log has no separate effort field.
 */
export async function expectRequestSetting(
  expect: ExpectStatic,
  admin: GatewayAdminClient,
  logs: RequestLog[],
  requested: RequestedSetting,
): Promise<string> {
  const effortField = "effort" in requested ? EFFORT_FIELDS[requested.operation] : undefined;
  const requests = await Promise.all(
    logs.map(async (log) => {
      const detail = await admin.getRequestLogDetail(log.request_log_id);
      const { body } = StoredRequestSchema.parse(detail.payload?.request_json);
      return {
        log,
        effort: effortField && valueAt(body, effortField),
        evidence: Object.fromEntries(
          Object.entries(body).filter(([key]) => !Object.hasOwn(LEFT_OUT_OF_EVIDENCE, key)),
        ),
      };
    }),
  );
  expect(
    requests.map(({ log, effort }) =>
      effortField ? { service_tier: log.service_tier, effort } : { service_tier: log.service_tier },
    ),
    JSON.stringify(requests.map(({ evidence }) => evidence)),
  ).toEqual(
    requests.map(() =>
      "effort" in requested
        ? { service_tier: null, effort: requested.effort }
        : { service_tier: requested.tier },
    ),
  );
  const tiers = [
    ...new Set(
      logs.map((log) => `${log.service_tier ?? "none"}->${log.served_service_tier ?? "unreported"}`),
    ),
  ];
  const efforts = effortField
    ? ` | ${effortField.join(".")} ${[...new Set(requests.map(({ effort }) => String(effort)))].join(",")}`
    : "";
  return `tier ${tiers.join(",")}${efforts}`;
}

function valueAt(value: unknown, path: readonly string[]): unknown {
  return path.reduce<unknown>(
    (current, key) =>
      typeof current === "object" && current !== null
        ? (current as Record<string, unknown>)[key]
        : undefined,
    value,
  );
}

/**
 * Prints one summary line per run. It writes to stdout directly, because Vitest can drop console
 * output from concurrent tests. `setting` is the summary from `expectRequestSetting`.
 */
export function reportRun(
  label: string,
  run: HarnessRun,
  logs: RequestLog[],
  setting?: string,
): void {
  const tools = run.toolCalls
    .filter((toolCall) => toolCall.status === "completed")
    .map((toolCall) => toolCall.name);
  const operations = [...new Set(logs.map((log) => String(log.metadata.operation)))];
  const harnesses = [...new Set(logs.map((log) => log.agent_harness_key))];
  process.stdout.write(
    `Harness run: ${label} | operation ${operations.join(",")} | harness ${harnesses.join(",")} | requests ${logs.length} | tools ${tools.join(",") || "none"}${setting ? ` | ${setting}` : ""}\n`,
  );
}
