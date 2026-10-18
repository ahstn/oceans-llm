import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { matchResultMarkers, verifyNoMcpGrant } from "./mcp-canary.mjs";
import { MAX_CANDIDATES, validateCandidates } from "./mcp-candidates.mjs";
import { attemptScope, loadStackEnv, mcpEndpointFor } from "./mcp-browser.mjs";

const scripts = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = process.env.OCEANS_VERIFY_REPO_ROOT ?? path.resolve(scripts, "../../../..");

test("all-optional candidates fail before browser launch and leave a failing proof", async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), "oceans-mcp-config-"));
  try {
    const configPath = path.join(directory, "candidates.json");
    await fs.writeFile(configPath, JSON.stringify({ candidates: [{
      key: "optional", label: "Optional", server_url: "https://example.com/mcp",
      auth_mode: "none", required: false, call: { name: "read", arguments: {} },
    }] }));
    const result = spawnSync(process.execPath, [path.join(scripts, "drive-mcp.mjs")], {
      env: {
        ...process.env, OCEANS_VERIFY_REPO_ROOT: repoRoot,
        OCEANS_VERIFY_BASE_URL: "http://127.0.0.1:1",
        OCEANS_VERIFY_EVIDENCE_DIR: path.join(directory, "evidence"),
        OCEANS_VERIFY_RUN_ID: "optional-test", OCEANS_VERIFY_GATEWAY_VERSION: "test",
        OCEANS_VERIFY_MCP_CANDIDATES_FILE: configPath,
      },
      encoding: "utf8", timeout: 15_000,
    });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /At least one MCP candidate must be required/);
    const proof = JSON.parse(await fs.readFile(path.join(directory, "evidence", "mcp-proof.json"), "utf8"));
    assert.equal(proof.verdict, "fail");
    assert.deepEqual((await fs.readdir(path.join(directory, "evidence"))), ["mcp-proof.json"]);
  } finally {
    await fs.rm(directory, { recursive: true, force: true });
  }
});

const candidate = {
  key: "fixture", server: { id: "server-id", server_key: "fixture" },
  tool: { id: "tool-id", upstream_name: "read" },
};

async function withGateway({ catalogExposed, cleanupFails }, operation) {
  let cleanupCalls = 0;
  const server = http.createServer(async (request, response) => {
    response.setHeader("content-type", "application/json");
    if (request.method === "DELETE") {
      cleanupCalls += 1;
      response.writeHead(cleanupFails ? 500 : 204).end();
      return;
    }
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const rpc = JSON.parse(Buffer.concat(chunks).toString());
    if (rpc.method === "notifications/initialized") {
      response.writeHead(202).end();
      return;
    }
    const envelope = { jsonrpc: "2.0", id: rpc.id };
    if (rpc.method === "initialize") {
      response.setHeader("mcp-session-id", "fixture-session");
      envelope.result = { protocolVersion: "2025-03-26" };
    } else if (request.url === "/mcp") {
      envelope.result = { structuredContent: { total: catalogExposed ? 1 : 0, items: catalogExposed ? [{}] : [] } };
    } else {
      response.statusCode = 403;
      envelope.error = { code: -32001, message: "denied" };
    }
    response.end(JSON.stringify(envelope));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const actions = [];
  try {
    await operation({
      baseURL: `http://127.0.0.1:${server.address().port}`, rawKey: "fixture-key",
      apiKeyId: "key-id", candidates: [candidate], actions,
      adminJson: async (_page, requestPath) => ({ total: 1, items: [{
        request_id: new URL(requestPath, "http://localhost").searchParams.get("request_id"),
        server_id: "server-id", api_key_id: "key-id", tool_display_key: "read",
        status: "policy_denied", policy_result: "denied", error_code: "mcp_tool_not_granted",
      }] }),
    }, actions);
    assert.equal(cleanupCalls, 1);
  } finally {
    await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  }
}

test("a cleanup error preserves the failed grant assertion and records both classes", async () => {
  await withGateway({ catalogExposed: true, cleanupFails: true }, async (options, actions) => {
    await assert.rejects(verifyNoMcpGrant(options), { code: "ungranted_catalog_exposed" });
    assert.deepEqual(actions.map((action) => action.failureClass), ["ungranted_catalog_exposed", "session_cleanup_failed"]);
  });
});

test("cleanup failure still fails a successful grant check", async () => {
  await withGateway({ catalogExposed: false, cleanupFails: true }, async (options, actions) => {
    await assert.rejects(verifyNoMcpGrant(options), { code: "session_cleanup_failed" });
    assert.equal(actions.at(-1).failureClass, "session_cleanup_failed");
  });
});

test("successful grant checks close their session and return the denial proof", async () => {
  await withGateway({ catalogExposed: false, cleanupFails: false }, async (options) => {
    const proofs = await verifyNoMcpGrant(options);
    assert.equal(proofs.length, 1);
    assert.equal(proofs[0].status, "policy_denied");
  });
});

test("each driver attempt in a run gets its own record keys", () => {
  const runId = "20261014T205646Z-hdr471b-7448";
  const first = attemptScope(runId, 1_760_000_000_000);
  const retry = attemptScope(runId, 1_760_000_060_000);
  assert.notEqual(first.key, retry.key);
  assert.match(first.key, /hdr471b-7448-[a-z0-9]{5}$/);
  // Server keys are `verify-<scope>-<candidate>` and must stay within the gateway's 64 characters.
  assert.ok(first.key.length <= 30);
  assert.match(first.key, /^[a-z0-9-]+$/);
});

test("credential aliases come from the launch record, not the driver's environment", async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), "oceans-mcp-stack-env-"));
  try {
    const file = path.join(directory, "stack-env.json");
    await fs.writeFile(file, JSON.stringify({
      mcpDiscoveryAliases: ["OCEANS_MCP_DISCOVERY_EXA_VERIFY"], clientConfigBaseUrl: "http://127.0.0.1:38123",
    }));
    const driverEnv = { OCEANS_MCP_DISCOVERY_GITHUB_VERIFY: "unused-by-gateway" };
    const launched = await loadStackEnv(file, driverEnv);
    assert.equal(launched.source, "launch");
    assert.deepEqual([...launched.aliases], ["OCEANS_MCP_DISCOVERY_EXA_VERIFY"]);
    assert.equal(launched.clientConfigBaseUrl, "http://127.0.0.1:38123");

    const fallback = await loadStackEnv(path.join(directory, "missing.json"), driverEnv);
    assert.equal(fallback.source, "driver");
    assert.deepEqual([...fallback.aliases], ["OCEANS_MCP_DISCOVERY_GITHUB_VERIFY"]);
    assert.equal(fallback.clientConfigBaseUrl, null);
  } finally {
    await fs.rm(directory, { recursive: true, force: true });
  }
});

test("the expected MCP endpoint normalizes the base URL like the gateway", () => {
  assert.equal(mcpEndpointFor("http://127.0.0.1:38123"), "http://127.0.0.1:38123/mcp");
  assert.equal(mcpEndpointFor("https://gateway.example/"), "https://gateway.example/mcp");
  assert.equal(mcpEndpointFor("https://gateway.example/v1"), "https://gateway.example/mcp");
  assert.equal(mcpEndpointFor("https://gateway.example/v1/"), "https://gateway.example/mcp");
  assert.equal(mcpEndpointFor("https://gateway.example/llm"), "https://gateway.example/llm/mcp");
});

const exampleFile = path.join(scripts, "../examples/mcp-candidates.json");

test("the example candidate file passes validation and pairs each control with a positive", async () => {
  const example = JSON.parse(await fs.readFile(exampleFile, "utf8"));
  validateCandidates(example);
  const isControl = (candidate) => candidate.expect_tool_error || candidate.expect_discovery_error;
  const host = (candidate) => new URL(candidate.server_url).host;
  const positives = new Set(example.candidates.filter((candidate) => !isControl(candidate)).map(host));
  const controls = example.candidates.filter(isControl).map(host);
  assert.deepEqual(controls.sort(), ["mcp.exa.ai", "mcp.reui.io"]);
  assert.ok(controls.every((controlHost) => positives.has(controlHost)));
});

function fixtureCandidate(overrides = {}) {
  return {
    key: "fixture", label: "Fixture", server_url: "https://example.com/mcp",
    auth_mode: "gateway_bearer_token", auth_config: { secret_ref: "env/OCEANS_MCP_DISCOVERY_FIXTURE_VERIFY" },
    call: { name: "read", arguments: {} }, ...overrides,
  };
}

test("candidate validation bounds the file and rejects ambiguous controls", () => {
  const many = Array.from({ length: MAX_CANDIDATES + 1 }, (_, index) => fixtureCandidate({ key: `fixture-${index}` }));
  assert.throws(() => validateCandidates({ candidates: many }), /one to 6 MCP candidates/);
  assert.throws(() => validateCandidates({ candidates: [fixtureCandidate({ expect_discovery_error: true })] }), /makes no tool call/);
  assert.throws(() => validateCandidates({ candidates: [fixtureCandidate({ expect_discovery_error: true, expect_tool_error: true, call: undefined })] }), /not both/);
  assert.throws(() => validateCandidates({ candidates: [fixtureCandidate({ expect_tool_error: true, expect_result_markers: ["ok"] })] }), /only to successful calls/);
  assert.throws(() => validateCandidates({ candidates: [fixtureCandidate({ expect_result_markers: [] })] }), /one to 4 short strings/);
  assert.throws(() => validateCandidates({ candidates: [fixtureCandidate({ auth_mode: "none", auth_config: {}, expect_discovery_error: true, call: undefined })] }), /authenticated server mode/);
  validateCandidates({ candidates: [fixtureCandidate(), fixtureCandidate({ key: "fixture-invalid", expect_discovery_error: true, call: undefined })] });
});

test("result markers match JSON carried in text content and fail without leaking it", () => {
  const result = { content: [{ type: "text", text: JSON.stringify({ plan: "pro", results: [{ free: false }] }) }] };
  assert.equal(matchResultMarkers(result, ['"plan":"pro"', '"free":false']), 2);
  assert.equal(matchResultMarkers(result), 0);
  assert.equal(matchResultMarkers({ content: [], structuredContent: { plan: "pro" } }, ['"plan":"pro"']), 1);
  assert.throws(() => matchResultMarkers(result, ['"plan":"free"']), (error) => error.code === "result_marker_missing" && !error.message.includes("pro"));
});
