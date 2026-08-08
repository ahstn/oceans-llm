import { z } from "zod";

import { delay } from "./process.js";
import type { GatewayRuntime } from "./types.js";

const RequestLogSchema = z.object({
  model_key: z.string(),
  provider_key: z.string(),
  request_log_id: z.string(),
  status_code: z.number().nullable(),
});

const RequestLogPageSchema = z.object({
  items: z.array(RequestLogSchema),
});

const RequestLogDetailSchema = z.object({
  log: RequestLogSchema,
  payload: z
    .object({
      request_json: z.unknown(),
      response_json: z.unknown(),
    })
    .nullable(),
  attempts: z.array(
    z.object({
      provider_key: z.string(),
      route_id: z.string(),
      status_code: z.number().nullable(),
      upstream_model: z.string(),
    }),
  ),
});

type RequestLog = z.infer<typeof RequestLogSchema>;
export type RequestLogDetail = z.infer<typeof RequestLogDetailSchema>;

export class GatewayAdminClient {
  readonly #runtime: GatewayRuntime;
  #cookie: string | undefined;

  constructor(runtime: GatewayRuntime) {
    this.#runtime = runtime;
  }

  async login(): Promise<void> {
    const response = await fetchWithTimeout(
      `${this.#runtime.baseUrl}/api/v1/auth/login/password`,
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          email: this.#runtime.adminEmail,
          password: this.#runtime.adminPassword,
        }),
      },
    );
    await assertSuccessful(response, "admin login");
    const setCookie = response.headers.get("set-cookie");
    if (!setCookie) {
      throw new Error("Gateway admin login did not return a session cookie");
    }
    this.#cookie = setCookie.split(";", 1)[0];
  }


  async waitForSuccessfulModelLog(requestTag: string): Promise<RequestLog> {
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline) {
      const page = await this.#getRequestLogs(requestTag, Math.max(1, deadline - Date.now()));
      const log = page.items.find((item) => item.status_code === 200);
      if (log) {
        return log;
      }
      await delay(250);
    }

    throw new Error(
      `No successful request log appeared for Oceans model ${this.#runtime.model} and harness tag ${requestTag}`,
    );
  }

  async getRequestLogDetail(requestLogId: string): Promise<RequestLogDetail> {
    if (!this.#cookie) {
      throw new Error("GatewayAdminClient.login() must be called first");
    }
    const response = await fetchWithTimeout(
      `${this.#runtime.baseUrl}/api/v1/admin/observability/request-logs/${requestLogId}`,
      { headers: { cookie: this.#cookie } },
    );
    return readEnvelope(response, "request-log detail", RequestLogDetailSchema);
  }

  async #getRequestLogs(
    requestTag: string,
    timeoutMs: number,
  ): Promise<z.infer<typeof RequestLogPageSchema>> {
    if (!this.#cookie) {
      throw new Error("GatewayAdminClient.login() must be called first");
    }
    const url = new URL("/api/v1/admin/observability/request-logs", this.#runtime.baseUrl);
    url.searchParams.set("page_size", "100");
    url.searchParams.set("model_key", this.#runtime.model);
    url.searchParams.set("tag_key", "harness_run");
    url.searchParams.set("tag_value", requestTag);
    const response = await fetchWithTimeout(
      url,
      { headers: { cookie: this.#cookie } },
      timeoutMs,
    );
    return readEnvelope(response, "request-log query", RequestLogPageSchema);
  }
}

export function fetchWithTimeout(
  input: string | URL,
  init: RequestInit = {},
  timeoutMs = 10_000,
): Promise<Response> {
  return fetch(input, { ...init, signal: AbortSignal.timeout(timeoutMs) });
}

export async function readEnvelope<T>(
  response: Response,
  operation: string,
  schema: z.ZodType<T>,
): Promise<T> {
  await assertSuccessful(response, operation);
  const body: unknown = await response.json();
  return z.object({ data: schema }).parse(body).data;
}

export async function assertSuccessful(response: Response, operation: string): Promise<void> {
  if (response.ok) {
    return;
  }
  const body = await response.text();
  throw new Error(`${operation} failed with HTTP ${response.status}: ${body}`);
}
