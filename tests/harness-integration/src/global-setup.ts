import { spawn, type ChildProcess } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { TestProject } from "vitest/node";
import { z } from "zod";

import { assertSuccessful, fetchWithTimeout, readEnvelope } from "./gateway-client.js";
import { delay, runCommand } from "./process.js";
import type { GatewayRuntime } from "./types.js";

const GATEWAY_MODEL = "harness-openrouter";
const ALLOWLISTED_GATEWAY_MODEL = "harness-openrouter-user-allowlist";
const ALLOWLISTED_USER_EMAIL = "allowlisted.harness@example.com";
const DEFAULT_OPENROUTER_MODEL = "deepseek/deepseek-v4-flash";
const MANAGED_API_KEY = "gwk_harness.integration-secret";
const MANAGED_ADMIN_EMAIL = "harness-admin@local";
const MANAGED_ADMIN_PASSWORD = "harness-admin-password";
const ROOT_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");

const McpServerSchema = z.object({
  server: z.object({ id: z.string() }),
});
const DiscoverySchema = z.object({
  status: z.string(),
  tools: z.array(z.object({ id: z.string() })),
});
const ServiceAccountsSchema = z.object({
  service_accounts: z.array(z.object({ id: z.string(), key: z.string() })),
});
const IdentityUsersSchema = z.object({
  users: z.array(z.object({ email: z.string(), id: z.string() })),
});
const CreatedApiKeySchema = z.object({
  raw_key: z.string(),
});
const PasswordInviteSchema = z.object({
  invite_url: z.string(),
});
const PasswordActivationSchema = z.object({
  status: z.literal("password_set"),
});

interface ManagedGateway {
  process: ChildProcess;
  runtimeDir: string;
}

export default async function setup(context: TestProject): Promise<() => Promise<void>> {
  const externalBaseUrl = process.env.GATEWAY_BASE_URL
    ? normalizeExternalBaseUrl(process.env.GATEWAY_BASE_URL)
    : undefined;
  if (externalBaseUrl) {
    const runtime = externalRuntime(externalBaseUrl);
    await assertGatewayReady(runtime.baseUrl);
    context.provide("gateway", runtime);
    return async () => undefined;
  }

  const openRouterApiKey = requiredEnvironment("OPENROUTER_API_KEY");
  const gatewayBinary = await buildGateway();
  const runtimeDir = await mkdtemp(join(tmpdir(), "oceans-harness-integration-"));
  let managed: ManagedGateway | undefined;
  try {
    const port = await availablePort();
    const baseUrl = `http://127.0.0.1:${port}`;
    const configPath = join(runtimeDir, "gateway.harness.yaml");
    const databasePath = join(runtimeDir, "gateway.harness.db");
    const upstreamModel = process.env.OPENROUTER_TEST_MODEL ?? DEFAULT_OPENROUTER_MODEL;
    await writeFile(configPath, gatewayConfig(port, databasePath, upstreamModel), "utf8");

    const runningGateway = startGateway(gatewayBinary, configPath, runtimeDir, openRouterApiKey);
    managed = runningGateway;
    const runtime: GatewayRuntime = {
      adminEmail: MANAGED_ADMIN_EMAIL,
      adminPassword: MANAGED_ADMIN_PASSWORD,
      apiKey: MANAGED_API_KEY,
      baseUrl,
      model: GATEWAY_MODEL,
    };
    await assertGatewayReady(baseUrl, runningGateway.process);
    runtime.allowlistedUser = {
      apiKey: await configureManagedGateway(runtime),
      model: ALLOWLISTED_GATEWAY_MODEL,
    };

    context.provide("gateway", runtime);
    return async () => stopManagedGateway(runningGateway);
  } catch (error) {
    if (managed) {
      await stopManagedGateway(managed);
    } else {
      await rm(runtimeDir, { force: true, recursive: true });
    }
    throw error;
  }
}

function externalRuntime(baseUrl: string): GatewayRuntime {
  const allowlistedUser = externalAllowlistedUser(
    process.env.OCEANS_ALLOWLISTED_USER_API_KEY,
    process.env.OCEANS_ALLOWLISTED_TEST_MODEL,
  );
  return {
    adminEmail: requiredEnvironment("GATEWAY_ADMIN_EMAIL"),
    adminPassword: requiredEnvironment("GATEWAY_ADMIN_PASSWORD"),
    apiKey: requiredEnvironment("OCEANS_API_KEY"),
    baseUrl,
    model: process.env.OCEANS_TEST_MODEL ?? GATEWAY_MODEL,
    ...(allowlistedUser ? { allowlistedUser } : {}),
  };
}

export function normalizeExternalBaseUrl(value: string): string {
  const url = new URL(value);
  const isLoopback = ["127.0.0.1", "[::1]", "::1", "localhost"].includes(url.hostname);
  if (url.protocol !== "https:" && !(url.protocol === "http:" && isLoopback)) {
    throw new Error("GATEWAY_BASE_URL must use HTTPS unless it targets a loopback host");
  }
  if (url.search || url.hash) {
    throw new Error("GATEWAY_BASE_URL must not include a query string or fragment");
  }
  url.pathname = url.pathname.replace(/\/+$/, "");
  return url.href.replace(/\/$/, "");
}

export function externalAllowlistedUser(
  apiKey: string | undefined,
  model: string | undefined,
): GatewayRuntime["allowlistedUser"] {
  if (Boolean(apiKey) !== Boolean(model)) {
    throw new Error(
      "OCEANS_ALLOWLISTED_USER_API_KEY and OCEANS_ALLOWLISTED_TEST_MODEL must be configured together",
    );
  }
  return apiKey && model ? { apiKey, model } : undefined;
}

function requiredEnvironment(name: string): string {
  const value = process.env[name];
  if (!value) {
    throw new Error(`${name} is required for the harness integration suite`);
  }
  return value;
}

async function buildGateway(): Promise<string> {
  const mise = process.env.MISE_BIN ?? "mise";
  await runCommand(mise, ["exec", "--", "cargo", "build", "-p", "gateway", "--bin", "gateway"], {
    cwd: ROOT_DIR,
    timeoutMs: 600_000,
  });
  const metadata = await runCommand(
    mise,
    ["exec", "--", "cargo", "metadata", "--format-version", "1", "--no-deps"],
    { cwd: ROOT_DIR, timeoutMs: 60_000 },
  );
  const parsed: unknown = JSON.parse(metadata.stdout);
  const targetDirectory = z.object({ target_directory: z.string() }).parse(parsed).target_directory;
  return join(targetDirectory, "debug", "gateway");
}

function startGateway(
  gatewayBinary: string,
  configPath: string,
  runtimeDir: string,
  openRouterApiKey: string,
): ManagedGateway {
  const gatewayProcess = spawn(gatewayBinary, [], {
    cwd: ROOT_DIR,
    env: {
      ...process.env,
      GATEWAY_CONFIG: configPath,
      GATEWAY_IDENTITY_TOKEN_SECRET: "harness-integration-identity-secret",
      HARNESS_OCEANS_API_KEY: MANAGED_API_KEY,
      OCEANS_API_KEY_SECRET_ENCRYPTION_KEY:
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
      OPENROUTER_API_KEY: openRouterApiKey,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  gatewayProcess.stdout?.pipe(process.stdout);
  gatewayProcess.stderr?.pipe(process.stderr);
  return { process: gatewayProcess, runtimeDir };
}

async function assertGatewayReady(baseUrl: string, gatewayProcess?: ChildProcess): Promise<void> {
  const deadline = Date.now() + 60_000;
  do {
    if (gatewayProcess?.signalCode) {
      throw new Error(`Managed Oceans gateway exited from signal ${gatewayProcess.signalCode}`);
    }
    if (gatewayProcess?.exitCode !== null && gatewayProcess?.exitCode !== undefined) {
      throw new Error(`Managed Oceans gateway exited with code ${gatewayProcess.exitCode}`);
    }
    try {
      const response = await fetchWithTimeout(
        `${baseUrl}/readyz`,
        {},
        Math.min(1_000, Math.max(1, deadline - Date.now())),
      );
      if (response.ok) {
        return;
      }
    } catch {
      // The gateway has not bound its socket yet.
    }
    await delay(250);
  } while (Date.now() < deadline);
  throw new Error(`Oceans gateway did not become ready at ${baseUrl}`);
}

async function configureManagedGateway(runtime: GatewayRuntime): Promise<string> {
  const login = await fetchWithTimeout(`${runtime.baseUrl}/api/v1/auth/login/password`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ email: runtime.adminEmail, password: runtime.adminPassword }),
  });
  await assertSuccessful(login, "managed gateway admin login");
  const cookie = login.headers.get("set-cookie")?.split(";", 1)[0];
  if (!cookie) {
    throw new Error("Managed gateway admin login did not return a session cookie");
  }

  const server = await adminRequest(
    runtime.baseUrl,
    cookie,
    "/api/v1/admin/mcp/servers",
    {
      auth_config: {},
      auth_mode: "none",
      description: "Context7 for native coding-harness integration tests",
      display_name: "Context7",
      recommended_catalog_key: null,
      server_key: "context7",
      server_url: "https://mcp.context7.com/mcp",
      timeout_ms: 30_000,
      transport: "streamable_http",
    },
    McpServerSchema,
  );
  const discovery = await adminRequest(
    runtime.baseUrl,
    cookie,
    `/api/v1/admin/mcp/servers/${server.server.id}/discovery-refresh`,
    undefined,
    DiscoverySchema,
  );
  if (discovery.status !== "success" || discovery.tools.length === 0) {
    throw new Error(`Context7 discovery failed: ${discovery.status}`);
  }

  const accountsResponse = await fetchWithTimeout(
    `${runtime.baseUrl}/api/v1/admin/identity/service-accounts`,
    { headers: { cookie } },
  );
  const accounts = await readEnvelope(
    accountsResponse,
    "service-account query",
    ServiceAccountsSchema,
  );
  const serviceAccount = accounts.service_accounts.find((account) => account.key === "harness");
  if (!serviceAccount) {
    throw new Error("Managed gateway did not seed the harness service account");
  }

  for (const tool of discovery.tools) {
    await adminRequest(
      runtime.baseUrl,
      cookie,
      "/api/v1/admin/mcp/grants",
      {
        subject_id: serviceAccount.id,
        subject_kind: "service_account",
        target_id: tool.id,
        target_kind: "tool",
      },
      z.unknown(),
      "PUT",
    );
  }

  const identity = await adminRequest(
    runtime.baseUrl,
    cookie,
    "/api/v1/admin/identity/users",
    undefined,
    IdentityUsersSchema,
    "GET",
  );
  const allowlistedUser = identity.users.find(
    (user) => user.email.toLowerCase() === ALLOWLISTED_USER_EMAIL,
  );
  if (!allowlistedUser) {
    throw new Error("Managed gateway did not seed the allowlisted human user");
  }
  const invitation = await adminRequest(
    runtime.baseUrl,
    cookie,
    `/api/v1/admin/identity/users/${allowlistedUser.id}/password-invite`,
    undefined,
    PasswordInviteSchema,
  );
  const invitationToken = new URL(invitation.invite_url, runtime.baseUrl)
    .pathname.split("/")
    .filter(Boolean)
    .pop();
  if (!invitationToken) {
    throw new Error("Managed gateway returned an invalid password invitation URL");
  }
  await adminRequest(
    runtime.baseUrl,
    cookie,
    `/api/v1/auth/invitations/${encodeURIComponent(invitationToken)}/password`,
    { password: "harness-allowlisted-user-password" },
    PasswordActivationSchema,
  );
  const createdKey = await adminRequest(
    runtime.baseUrl,
    cookie,
    "/api/v1/admin/api-keys",
    {
      model_grant_mode: "all",
      model_keys: [],
      name: "Harness Allowlisted User Key",
      owner_kind: "user",
      owner_service_account_id: null,
      owner_team_id: null,
      owner_user_id: allowlistedUser.id,
    },
    CreatedApiKeySchema,
  );
  return createdKey.raw_key;
}

async function adminRequest<T>(
  baseUrl: string,
  cookie: string,
  path: string,
  body: unknown,
  schema: z.ZodType<T>,
  method = "POST",
): Promise<T> {
  const request: RequestInit = {
    method,
    headers: { "content-type": "application/json", cookie },
  };
  if (body !== undefined) {
    request.body = JSON.stringify(body);
  }
  const response = await fetchWithTimeout(`${baseUrl}${path}`, request);
  return readEnvelope(response, path, schema);
}

async function availablePort(): Promise<number> {
  const server = createServer();
  const { promise, resolve: listening, reject } = Promise.withResolvers<void>();
  server.once("error", reject);
  server.listen(0, "127.0.0.1", listening);
  await promise;
  const address = server.address();
  if (!address || typeof address === "string") {
    server.close();
    throw new Error("Failed to allocate a local gateway port");
  }
  const { promise: closed, resolve, reject: rejectClose } = Promise.withResolvers<void>();
  server.close((error) => {
    if (error) {
      rejectClose(error);
    } else {
      resolve();
    }
  });
  await closed;
  return address.port;
}

async function stopManagedGateway(managed: ManagedGateway): Promise<void> {
  if (!hasProcessExited(managed.process)) {
    managed.process.kill("SIGTERM");
    if (!(await waitForProcessExit(managed.process, 5_000))) {
      managed.process.kill("SIGKILL");
      if (!(await waitForProcessExit(managed.process, 5_000))) {
        throw new Error("Managed gateway did not exit after SIGKILL");
      }
    }
  }
  await rm(managed.runtimeDir, { force: true, recursive: true });
}

function hasProcessExited(process: ChildProcess): boolean {
  return process.exitCode !== null || process.signalCode !== null;
}

function waitForProcessExit(process: ChildProcess, timeoutMs: number): Promise<boolean> {
  if (hasProcessExited(process)) {
    return Promise.resolve(true);
  }
  return new Promise((resolve) => {
    let timer: NodeJS.Timeout | undefined;
    let settled = false;
    const finish = (exited: boolean) => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);
      process.off("exit", onExit);
      resolve(exited);
    };
    const onExit = () => finish(true);
    process.once("exit", onExit);
    if (hasProcessExited(process)) {
      finish(true);
      return;
    }
    timer = setTimeout(() => finish(false), timeoutMs);
  });
}

function gatewayConfig(port: number, databasePath: string, upstreamModel: string): string {
  return `server:
  bind: "127.0.0.1:${port}"
  log_format: "pretty"

database:
  path: ${JSON.stringify(databasePath)}

auth:
  bootstrap_admin:
    enabled: true
    email: ${MANAGED_ADMIN_EMAIL}
    password: literal.${MANAGED_ADMIN_PASSWORD}
    require_password_change: false

request_logging:
  payloads:
    capture_mode: redacted_payloads
    request_max_bytes: 65536
    response_max_bytes: 65536
    stream_max_events: 128
    redaction_paths: []

teams:
  - id: harness
    name: Harness Integration

users:
  - name: Allowlisted Harness User
    email: ${ALLOWLISTED_USER_EMAIL}
    auth_mode: password

service_accounts:
  - id: harness
    name: Harness Integration
    team: harness
    budget:
      cadence: daily
      amount_usd: "5.0000"
      hard_limit: true
      timezone: UTC
    keys:
      - id: integration
        name: Harness Integration Key
        value: env.HARNESS_OCEANS_API_KEY
        allowed_models: [${GATEWAY_MODEL}]

providers:
  - id: openrouter
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    pricing_provider_id: openrouter
    auth:
      kind: bearer
      token: env.OPENROUTER_API_KEY

models:
  - id: ${GATEWAY_MODEL}
    description: DeepSeek V4 Flash through OpenRouter for harness integration tests
    routes:
      - provider: openrouter
        upstream_model: ${JSON.stringify(upstreamModel)}

  - id: ${ALLOWLISTED_GATEWAY_MODEL}
    description: Mixed-case human-user allowlist through OpenRouter
    allowlist:
      users:
        - Allowlisted.Harness@Example.com
    routes:
      - provider: openrouter
        upstream_model: ${JSON.stringify(upstreamModel)}
`;
}
