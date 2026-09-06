import { randomUUID } from "node:crypto";

const REQUEST_TIMEOUT_MS = 30_000;
const MAX_RESPONSE_BYTES = 2 * 1024 * 1024;
const MAX_LIST_PAGES = 8;
const INITIAL_PROTOCOL = "2025-03-26";

// Only fixed failure classes leave this module. Fetch and browser exceptions can
// contain response bodies, URLs, or page content and must not enter evidence.
class CanaryFailure extends Error {
  constructor(code) {
    super(code);
    this.code = code;
  }
}

function requireCondition(condition, code) {
  if (!condition) throw new CanaryFailure(code);
}

function failureClass(error) {
  return error instanceof CanaryFailure ? error.code : "transport_or_browser_failure";
}

function gatewayEndpoint(baseURL, serverKey) {
  const base = new URL(baseURL);
  requireCondition(
    ["127.0.0.1", "localhost", "[::1]"].includes(base.hostname) &&
      !base.username && !base.password && !base.search && !base.hash,
    "non_local_gateway_rejected",
  );
  return new URL(serverKey ? `/mcp/${encodeURIComponent(serverKey)}` : "/mcp", base);
}

function matchingEnvelope(value, id) {
  if (!value || typeof value !== "object" || value.id !== id) return undefined;
  requireCondition(value.jsonrpc === "2.0", "invalid_jsonrpc_version");
  requireCondition(
    Object.hasOwn(value, "result") !== Object.hasOwn(value, "error"),
    "invalid_jsonrpc_envelope",
  );
  return value;
}

function parseJson(text) {
  try {
    return JSON.parse(text);
  } catch {
    throw new CanaryFailure("invalid_json_response");
  }
}

function sseEnvelope(text, id) {
  const normalized = text.replace(/^\uFEFF/, "").replace(/\r\n?/g, "\n");
  const completed = normalized.split("\n\n");
  completed.pop(); // An SSE event is dispatched only after its blank line.
  for (const event of completed) {
    const data = event.split("\n")
      .filter((line) => line === "data" || line.startsWith("data:"))
      .map((line) => line === "data" ? "" : line.slice(5).replace(/^ /, ""))
      .join("\n");
    if (!data) continue;
    const envelope = matchingEnvelope(parseJson(data), id);
    if (envelope) return envelope;
  }
  return undefined;
}

async function readRpcResponse(response, id) {
  requireCondition(response.body, "missing_response_body");
  const isSse = response.headers.get("content-type")?.toLowerCase().startsWith("text/event-stream");
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let bytes = 0;
  let text = "";
  try {
    const declaredBytes = Number(response.headers.get("content-length"));
    requireCondition(!declaredBytes || declaredBytes <= MAX_RESPONSE_BYTES, "response_byte_limit");
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      bytes += value.byteLength;
      requireCondition(bytes <= MAX_RESPONSE_BYTES, "response_byte_limit");
      text += decoder.decode(value, { stream: true });
      if (isSse) {
        const envelope = sseEnvelope(text, id);
        if (envelope) return { envelope, bytes, transport: "sse" };
      }
    }
    text += decoder.decode();
    const envelope = isSse ? sseEnvelope(text, id) : matchingEnvelope(parseJson(text), id);
    requireCondition(envelope, "matching_response_missing");
    return { envelope, bytes, transport: isSse ? "sse" : "json" };
  } finally {
    await reader.cancel().catch(() => {});
  }
}

function client(baseURL, rawKey, serverKey) {
  const endpoint = gatewayEndpoint(baseURL, serverKey);
  let session;
  let protocol = INITIAL_PROTOCOL;

  async function send(method, params, { id = randomUUID(), expectedStatus = 200, notification = false } = {}) {
    const response = await fetch(endpoint, {
      method: "POST",
      redirect: "error",
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
      headers: {
        authorization: `Bearer ${rawKey}`,
        "content-type": "application/json",
        accept: "application/json, text/event-stream",
        "mcp-protocol-version": protocol,
        ...(session ? { "mcp-session-id": session } : {}),
      },
      body: JSON.stringify({ jsonrpc: "2.0", ...(notification ? {} : { id }), method, params }),
    });
    if (response.status !== expectedStatus && !(notification && response.ok)) {
      await response.body?.cancel().catch(() => {});
      throw new CanaryFailure(`unexpected_http_${response.status}`);
    }
    if (notification) {
      await response.body?.cancel().catch(() => {});
      return { status: response.status };
    }
    if (method === "initialize") {
      // Keep the issued token even when the body is malformed so finally can
      // revoke the session without putting that token in a proof artifact.
      session = response.headers.get("mcp-session-id") || undefined;
    }
    const result = await readRpcResponse(response, id);
    if (method === "initialize") {
      const negotiated = result.envelope.result?.protocolVersion;
      requireCondition(typeof negotiated === "string" && /^\d{4}-\d{2}-\d{2}$/.test(negotiated), "invalid_negotiated_protocol");
      protocol = negotiated;
    }
    return { ...result, requestId: id, status: response.status };
  }

  return {
    send,
    async initialize() {
      const response = await send("initialize", {
        protocolVersion: INITIAL_PROTOCOL,
        capabilities: {},
        clientInfo: { name: "oceans-admin-verification", version: "1.0.0" },
      });
      requireCondition(!response.envelope.error, "initialize_rpc_error");
      requireCondition(serverKey || session, "aggregate_session_missing");
      await send("notifications/initialized", undefined, { notification: true });
      return { protocol, hasSession: Boolean(session), transport: response.transport };
    },
    async close() {
      if (!session) return { status: "not_applicable" };
      const response = await fetch(endpoint, {
        method: "DELETE",
        redirect: "error",
        signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
        headers: {
          authorization: `Bearer ${rawKey}`,
          "mcp-session-id": session,
          "mcp-protocol-version": protocol,
        },
      });
      await response.body?.cancel().catch(() => {});
      requireCondition(response.ok || (serverKey && response.status === 405), "session_cleanup_failed");
      session = undefined;
      return { status: response.status, termination: response.status === 405 ? "upstream_termination_unsupported" : "confirmed" };
    },
  };
}

function successfulResult(response) {
  requireCondition(!response.envelope.error, "rpc_error");
  const result = response.envelope.result;
  requireCondition(result && typeof result === "object", "missing_rpc_result");
  requireCondition(result.isError !== true, "upstream_tool_error");
  return result;
}

function expectedAuthenticationFailure(response) {
  requireCondition(!response.envelope.error, "expected_tool_error_got_rpc_error");
  const result = response.envelope.result;
  requireCondition(result?.isError === true, "expected_authentication_error_missing");
  // Inspect nested structured content and text only in memory. Never return the
  // upstream message, because authentication errors can echo credentials.
  const marker = /\b(?:401|403|unauthorized|unauthenticated)\b|\b(?:invalid|missing|expired)(?:\s+or\s+missing)?(?:\s+(?:api|access))?[\s_-]+(?:key|token)\b|\bauthentication\b/i;
  requireCondition(marker.test(JSON.stringify(result)), "authentication_error_marker_missing");
  return result;
}

async function listedTools(connection) {
  const tools = [];
  let cursor;
  for (let page = 0; page < MAX_LIST_PAGES; page += 1) {
    const result = successfulResult(await connection.send("tools/list", cursor ? { cursor } : {}));
    requireCondition(Array.isArray(result.tools), "invalid_tools_list");
    tools.push(...result.tools);
    if (!result.nextCursor) return tools;
    requireCondition(typeof result.nextCursor === "string" && result.nextCursor !== cursor, "invalid_tools_cursor");
    cursor = result.nextCursor;
  }
  throw new CanaryFailure("tools_list_page_limit");
}

async function invocationProof({ page, adminJson, candidate, apiKeyId, requestId, expectedStatus = "success" }) {
  const query = new URLSearchParams({ request_id: requestId, page_size: "10" });
  const records = await adminJson(page, `/api/v1/admin/observability/mcp-invocations?${query}`);
  requireCondition(records.total === 1 && records.items.length === 1, "invocation_record_count");
  const row = records.items[0];
  requireCondition(row.request_id === requestId && row.server_id === candidate.server.id && row.api_key_id === apiKeyId &&
    row.tool_display_key === candidate.tool.upstream_name && row.status === expectedStatus,
  "invocation_record_mismatch");
  if (expectedStatus !== "policy_denied") {
    requireCondition(row.tool_id === candidate.tool.id && row.policy_result === "allowed", "invocation_tool_or_policy_mismatch");
  } else {
    requireCondition(row.policy_result === "denied" && row.error_code === "mcp_tool_not_granted", "invocation_denial_mismatch");
  }
  if (expectedStatus === "upstream_error") {
    requireCondition(row.metadata?.mcp_route === "aggregate" && row.metadata?.aggregate_tool === "call_tool", "invocation_aggregate_metadata_mismatch");
  }
  return {
    requestId,
    invocationId: row.mcp_tool_invocation_id,
    serverId: row.server_id,
    toolId: row.tool_id,
    apiKeyId: row.api_key_id,
    status: row.status,
    policyResult: row.policy_result,
    hasPayload: row.has_payload,
    latencyMs: row.latency_ms,
  };
}

async function verifyInvocationUi({ page, baseURL, proof, capture, name }) {
  await page.goto(new URL("/admin/observability/mcp-invocations", baseURL).href, { waitUntil: "networkidle" });
  await page.getByRole("heading", { name: "MCP invocations", exact: true }).waitFor();
  await page.getByTestId("mcp-filter-request-id").fill(proof.requestId);
  await page.getByRole("button", { name: "Apply Filters", exact: true }).click();
  const row = page.getByTestId("mcp-invocations-table").getByRole("row").filter({ hasText: proof.invocationId });
  await row.waitFor();
  requireCondition(await row.count() === 1, "invocation_ui_row_count");
  // The screenshot is taken before opening details, which contain tool payloads.
  await capture(page, name);
  await row.getByRole("button", { name: "Inspect", exact: true }).click();
  const detail = page.getByTestId("mcp-invocation-detail");
  await detail.getByText(proof.invocationId, { exact: true }).waitFor();
  await detail.getByText(proof.requestId, { exact: true }).waitFor();
  await detail.getByRole("button", { name: "Close", exact: true }).click();
  return { listMatched: true, detailMatched: true };
}

async function candidateCanary(options, candidate) {
  const { page, baseURL, rawKey, apiKeyId, adminJson, actions, capture } = options;
  const direct = client(baseURL, rawKey, candidate.server.server_key);
  const aggregate = client(baseURL, rawKey);
  const proof = {
    candidate: candidate.key,
    serverId: candidate.server.id,
    toolId: candidate.tool.id,
    expectedToolError: candidate.expect_tool_error === true,
  };
  try {
    requireCondition(candidate.call.name === candidate.tool.upstream_name, "candidate_call_mismatch");
    if (!candidate.expect_tool_error) {
      proof.directInitialize = await direct.initialize();
      const tools = await listedTools(direct);
      requireCondition(tools.length === 1 && tools[0].name === candidate.tool.upstream_name, "direct_grant_filter_mismatch");
      proof.directVisibleToolCount = tools.length;
      const directCall = await direct.send("tools/call", candidate.call);
      const directResult = successfulResult(directCall);
      requireCondition(Array.isArray(directResult.content) && directResult.content.length > 0, "empty_tool_result");
      proof.direct = await invocationProof({ page, adminJson, candidate, apiKeyId, requestId: directCall.requestId });
      proof.direct.transport = directCall.transport;
      proof.direct.contentCount = directResult.content.length;
      actions.push({ action: "direct MCP tool call and invocation record", result: `${candidate.key}: success` });
    }

    proof.aggregateInitialize = await aggregate.initialize();
    const builtins = await listedTools(aggregate);
    requireCondition(builtins.map((tool) => tool.name).sort().join(",") === "call_tool,describe_tool,search_tools", "aggregate_builtin_mismatch");
    const search = successfulResult(await aggregate.send("tools/call", {
      name: "search_tools", arguments: { query: "", server_key: candidate.server.server_key, limit: 50 },
    })).structuredContent;
    requireCondition(search?.total === 1 && search.items?.length === 1 &&
      search.items[0].tool.mcp_tool_id === candidate.tool.id &&
      search.items[0].server.mcp_server_id === candidate.server.id,
    "aggregate_grant_filter_mismatch");
    const address = search.items[0].address;
    const described = successfulResult(await aggregate.send("tools/call", {
      name: "describe_tool", arguments: { address },
    })).structuredContent;
    requireCondition(described?.address === address && described.tool?.mcp_tool_id === candidate.tool.id &&
      typeof described.tool.schema_hash === "string", "aggregate_description_mismatch");
    const aggregateCall = await aggregate.send("tools/call", {
      name: "call_tool",
      arguments: { address, arguments: candidate.call.arguments, schema_hash: described.tool.schema_hash },
    });
    const aggregateResult = candidate.expect_tool_error
      ? expectedAuthenticationFailure(aggregateCall)
      : successfulResult(aggregateCall);
    requireCondition(Array.isArray(aggregateResult.content) && aggregateResult.content.length > 0, "empty_tool_result");
    proof.aggregate = await invocationProof({
      page, adminJson, candidate, apiKeyId, requestId: aggregateCall.requestId,
      expectedStatus: candidate.expect_tool_error ? "upstream_error" : "success",
    });
    if (candidate.expect_tool_error) proof.aggregate.authenticationErrorObserved = true;
    proof.aggregate.transport = aggregateCall.transport;
    proof.aggregate.contentCount = aggregateResult.content.length;
    proof.aggregateVisibleToolCount = search.items.length;
    proof.aggregateBuiltinCount = builtins.length;
    proof.ui = await verifyInvocationUi({ page, baseURL, proof: proof.aggregate, capture, name: `mcp-${candidate.key}-invocation` });
    actions.push({
      action: "aggregate MCP catalog, tool call, and invocation UI",
      result: `${candidate.key}: ${candidate.expect_tool_error ? "authentication failure confirmed" : "success"}`,
    });
    proof.status = "passed";
  } catch (error) {
    proof.status = "failed";
    proof.failureClass = failureClass(error);
  } finally {
    proof.sessionCleanup = [];
    for (const connection of [direct, aggregate]) {
      try { proof.sessionCleanup.push(await connection.close()); }
      catch (error) {
        proof.sessionCleanup.push({ status: "failed", failureClass: failureClass(error) });
        proof.status = "failed";
        proof.failureClass ??= "session_cleanup_failed";
      }
    }
  }
  return proof;
}

export async function runMcpCanaries(options) {
  const proofs = [];
  for (const candidate of options.candidates) proofs.push(await candidateCanary(options, candidate));
  return {
    candidates: proofs,
    failures: proofs.filter((proof) => proof.status === "failed")
      .map((proof) => ({ candidate: proof.candidate, reasonClass: proof.failureClass })),
  };
}

// Run before adding grants. The direct denied call requires no upstream session:
// handle_tools_call rejects it before credential preparation or tool execution.
export async function verifyNoMcpGrant({ page, baseURL, rawKey, apiKeyId, candidates, adminJson, actions }) {
  const aggregate = client(baseURL, rawKey);
  const proofs = [];
  let verificationFailed = false;
  try {
    await aggregate.initialize();
    for (const candidate of candidates) {
      const search = successfulResult(await aggregate.send("tools/call", {
        name: "search_tools", arguments: { query: "", server_key: candidate.server.server_key },
      })).structuredContent;
      requireCondition(search?.total === 0 && search.items?.length === 0, "ungranted_catalog_exposed");
      const direct = client(baseURL, rawKey, candidate.server.server_key);
      const denied = await direct.send("tools/call", {
        name: candidate.tool.upstream_name, arguments: {},
      }, { expectedStatus: 403 });
      requireCondition(denied.envelope.error?.code === -32001, "ungranted_call_not_denied");
      const log = await invocationProof({ page, adminJson, candidate, apiKeyId, requestId: denied.requestId, expectedStatus: "policy_denied" });
      proofs.push({ candidate: candidate.key, visibleToolCount: 0, httpStatus: denied.status, ...log });
      actions.push({ action: "reject MCP tool before grant", result: `${candidate.key}: policy denied` });
    }
  } catch (error) {
    verificationFailed = true;
    actions.push({ action: "verify MCP access before grant", result: "failed", failureClass: failureClass(error) });
    throw error;
  } finally {
    try { await aggregate.close(); }
    catch (error) {
      actions.push({ action: "close pre-grant aggregate session", result: "failed", failureClass: failureClass(error) });
      if (!verificationFailed) throw error;
    }
  }
  return proofs;
}

export async function testRevokedKey({ baseURL, rawKey, candidates = [] }) {
  const proofs = [];
  for (const serverKey of [undefined, ...candidates.map((candidate) => candidate.server.server_key)]) {
    const response = await fetch(gatewayEndpoint(baseURL, serverKey), {
      method: "POST",
      redirect: "error",
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
      headers: { authorization: `Bearer ${rawKey}`, "content-type": "application/json", accept: "application/json, text/event-stream" },
      body: JSON.stringify({ jsonrpc: "2.0", id: randomUUID(), method: "tools/list", params: {} }),
    });
    await response.body?.cancel().catch(() => {});
    requireCondition(response.status === 401, "revoked_key_not_rejected");
    proofs.push({ route: serverKey ? "direct" : "aggregate", status: response.status });
  }
  return proofs;
}
