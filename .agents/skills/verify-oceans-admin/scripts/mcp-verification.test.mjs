import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { verifyNoMcpGrant } from "./mcp-canary.mjs";

const scripts = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = process.env.OCEANS_VERIFY_REPO_ROOT ?? path.resolve(scripts, "../../../..");

test("all-optional candidates fail before browser launch or evidence creation", async () => {
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
    await assert.rejects(fs.access(path.join(directory, "evidence")), { code: "ENOENT" });
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
