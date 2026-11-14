import { z } from "zod";

import { delay } from "./process.js";
import type { GatewayRuntime } from "./types.js";

const RequestLogSchema = z.object({
  agent_harness_key: z.string(),
  metadata: z.record(z.string(), z.unknown()),
  model_key: z.string(),
  provider_key: z.string(),
  request_log_id: z.string(),
  status_code: z.number().nullable(),
  prompt_tokens: z.number().nullable(),
  completion_tokens: z.number().nullable(),
  cost_usd_10000: z.number().nullable(),
  service_tier: z.string().nullable(),
  served_service_tier: z.string().nullable(),
});

const RequestLogPageSchema = z.object({
  items: z.array(RequestLogSchema),
});

const RequestLogDetailSchema = z.object({
  log: RequestLogSchema,
  user_agent_raw: z.string().nullable(),
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

export type RequestLog = z.infer<typeof RequestLogSchema>;
export type RequestLogDetail = z.infer<typeof RequestLogDetailSchema>;
type AdminCredentials = Pick<GatewayRuntime, "adminEmail" | "adminPassword" | "baseUrl">;

const ClientConfigsSchema = z.object({
  client_configurations: z.array(
    z.object({
      key: z.string(),
      blocks: z.array(z.object({ content: z.string() })),
    }),
  ),
});

/**
 * Fetches the code blocks of the client config that Oceans generates for `model`, as the admin UI
 * shows them. `client` is the config key, such as `opencode`, `pi` or `claude-code`.
 */
export async function fetchGeneratedClientConfig(
  runtime: AdminCredentials,
  model: string,
  client: string,
): Promise<string[]> {
  const admin = new GatewayAdminClient(runtime);
  await admin.login();
  const { client_configurations } = await admin.generateClientConfigs([model]);
  const blocks = client_configurations.find((config) => config.key === client)?.blocks;
  if (!blocks?.length) {
    throw new Error(`Oceans generated no ${client} config for model ${model}`);
  }
  return blocks.map((block) => block.content);
}

export class GatewayAdminClient {
  readonly #runtime: AdminCredentials;
  #cookie: string | undefined;

  constructor(runtime: AdminCredentials) {
    this.#runtime = runtime;
  }

  async login(): Promise<void> {
    const response = await fetchWithTimeout(`${this.#runtime.baseUrl}/api/v1/auth/login/password`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        email: this.#runtime.adminEmail,
        password: this.#runtime.adminPassword,
      }),
    });
    await assertSuccessful(response, "admin login");
    const setCookie = response.headers.get("set-cookie");
    if (!setCookie) {
      throw new Error("Gateway admin login did not return a session cookie");
    }
    this.#cookie = setCookie.split(";", 1)[0];
  }

  async generateClientConfigs(modelKeys: string[]): Promise<z.infer<typeof ClientConfigsSchema>> {
    if (!this.#cookie) {
      throw new Error("GatewayAdminClient.login() must be called first");
    }
    const response = await fetchWithTimeout(
      `${this.#runtime.baseUrl}/api/v1/admin/models/client-configs`,
      {
        method: "POST",
        headers: { "content-type": "application/json", cookie: this.#cookie },
        body: JSON.stringify({ model_keys: modelKeys }),
      },
    );
    return readEnvelope(response, "model client configs", ClientConfigsSchema);
  }

  async waitForSuccessfulModelLog(requestTag: string, model: string): Promise<RequestLog> {
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline) {
      const page = await this.#getRequestLogs(
        requestTag,
        Math.max(1, deadline - Date.now()),
        model,
      );
      const log = page.items.find((item) => item.status_code === 200);
      if (log) {
        return log;
      }
      await delay(250);
    }

    throw new Error(
      `No successful request log appeared for Oceans model ${model} and harness tag ${requestTag}`,
    );
  }

  /**
   * Returns every request log of a harness run, for any model, once at least `minimum` are
   * recorded. Logs are written after their responses finish, so the query repeats once more after
   * a short wait to include a late log.
   */
  async waitForRequestLogs(requestTag: string, minimum: number): Promise<RequestLog[]> {
    const deadline = Date.now() + 30_000;
    let found = 0;
    while (Date.now() < deadline) {
      found = (await this.#getRequestLogs(requestTag, Math.max(1, deadline - Date.now()))).items
        .length;
      if (found >= minimum) {
        await delay(1_000);
        return (await this.#getRequestLogs(requestTag, 10_000)).items;
      }
      await delay(250);
    }
    throw new Error(
      `Expected at least ${minimum} request logs for harness tag ${requestTag}, found ${found}`,
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
    model?: string,
  ): Promise<z.infer<typeof RequestLogPageSchema>> {
    if (!this.#cookie) {
      throw new Error("GatewayAdminClient.login() must be called first");
    }
    const url = new URL("/api/v1/admin/observability/request-logs", this.#runtime.baseUrl);
    url.searchParams.set("page_size", "100");
    if (model) {
      url.searchParams.set("model_key", model);
    }
    url.searchParams.set("tag_key", "harness_run");
    url.searchParams.set("tag_value", requestTag);
    const response = await fetchWithTimeout(url, { headers: { cookie: this.#cookie } }, timeoutMs);
    return readEnvelope(response, "request-log query", RequestLogPageSchema);
  }
}

/**
 * Parses a URL that will receive an Oceans API key. Keys may only travel over HTTPS, or over HTTP to
 * a loopback host.
 */
export function parseCredentialUrl(value: string, label: string): URL {
  const url = new URL(value);
  const isLoopback = ["127.0.0.1", "[::1]", "::1", "localhost"].includes(url.hostname);
  if (url.protocol !== "https:" && !(url.protocol === "http:" && isLoopback)) {
    throw new Error(`${label} must use HTTPS unless it targets a loopback host`);
  }
  return url;
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
