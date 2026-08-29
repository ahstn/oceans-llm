import { expect, test, type APIRequestContext } from 'playwright/test'

import { ensureAdminSession } from './admin-session'

const gatewayApiKey = process.env.E2E_GATEWAY_API_KEY ?? 'gwk_e2e.secret-value'
const upstreamPort = process.env.E2E_UPSTREAM_PORT ?? '38081'
const upstreamRoot = `http://127.0.0.1:${upstreamPort}`

function chatRequest(user?: string, stream = false) {
  return {
    model: 'fast',
    messages: [{ role: 'user', content: 'Use the requested fixture.' }],
    stream,
    ...(user ? { user } : {}),
  }
}

async function upstreamRequestCount(request: APIRequestContext) {
  const response = await request.get(`${upstreamRoot}/__admin/requests`)
  expect(response.status()).toBe(200)
  const payload = (await response.json()) as { requests: unknown[] }
  return payload.requests.length
}

async function requestLogCount(
  request: APIRequestContext,
  root: string,
  cookie: string,
  statusCode: number,
) {
  const response = await request.get(
    `${root}/api/v1/admin/observability/request-logs?status_code=${statusCode}`,
    { headers: { cookie } },
  )
  expect(response.status()).toBe(200)
  const payload = (await response.json()) as { data: { total: number } }
  return payload.data.total
}

test('guards prompt, non-stream response, and buffered stream boundaries', async ({
  page,
  request,
  baseURL,
}) => {
  const root = baseURL ?? 'http://127.0.0.1:38080'
  const headers = {
    authorization: `Bearer ${gatewayApiKey}`,
    'content-type': 'application/json',
  }
  const cookie = await ensureAdminSession(page, request, root)
  const deniedLogsBefore = await requestLogCount(request, root, cookie, 403)
  await request.delete(`${upstreamRoot}/__admin/requests`)

  const promptDenied = await request.post(`${root}/v1/chat/completions`, {
    headers,
    data: {
      ...chatRequest(),
      messages: [{ role: 'user', content: 'guardrail-e2e-managed-deny' }],
    },
  })
  expect(promptDenied.status()).toBe(403)
  expect(await upstreamRequestCount(request)).toBe(0)

  for (const [path, data, extraHeaders] of [
    [
      '/v1/messages',
      {
        model: 'fast',
        max_tokens: 32,
        messages: [{ role: 'user', content: 'guardrail-e2e-managed-deny' }],
      },
      { 'anthropic-version': '2023-06-01' },
    ],
    [
      '/v1/responses',
      {
        model: 'fast',
        input: 'guardrail-e2e-managed-deny',
      },
      {},
    ],
  ] as const) {
    const denied = await request.post(`${root}${path}`, {
      headers: { ...headers, ...extraHeaders },
      data,
    })
    expect(denied.status(), path).toBe(403)
    expect(await upstreamRequestCount(request), path).toBe(0)
  }
  expect(await requestLogCount(request, root, cookie, 403)).toBeGreaterThanOrEqual(
    deniedLogsBefore + 3,
  )

  const auditAllowed = await request.post(`${root}/v1/chat/completions`, {
    headers,
    data: {
      ...chatRequest(),
      model: 'audit-fast',
      messages: [{ role: 'user', content: 'guardrail-e2e-managed-deny' }],
    },
  })
  expect(auditAllowed.status()).toBe(200)
  expect(await upstreamRequestCount(request)).toBe(1)

  const maskedPrompt = await request.post(`${root}/v1/chat/completions`, {
    headers,
    data: {
      ...chatRequest(),
      messages: [{ role: 'user', content: 'guardrail-e2e-mask' }],
    },
  })
  expect(maskedPrompt.status()).toBe(200)
  const captured = (await (await request.get(`${upstreamRoot}/__admin/requests`)).json()) as {
    requests: Array<{ body: unknown }>
  }
  expect(JSON.stringify(captured.requests.at(-1)?.body)).toContain('[masked]')
  expect(JSON.stringify(captured.requests.at(-1)?.body)).not.toContain('guardrail-e2e-mask')

  const failOpenPrompt = await request.post(`${root}/v1/chat/completions`, {
    headers,
    data: {
      ...chatRequest(),
      messages: [{ role: 'user', content: 'guardrail-e2e-fail-open' }],
    },
  })
  expect(failOpenPrompt.status()).toBe(200)

  const safe = await request.post(`${root}/v1/chat/completions`, {
    headers,
    data: chatRequest(),
  })
  expect(safe.status()).toBe(200)
  expect((await safe.json()).choices[0].message.content).toBe('pong')

  const responseDenied = await request.post(`${root}/v1/chat/completions`, {
    headers,
    data: chatRequest('guardrail-e2e-tool-call'),
  })
  expect(responseDenied.status()).toBe(403)
  expect(await responseDenied.text()).not.toContain('rm -rf')

  const safeStream = await request.post(`${root}/v1/chat/completions`, {
    headers,
    data: chatRequest(undefined, true),
  })
  expect(safeStream.status()).toBe(200)
  expect(await safeStream.text()).toContain('pong')

  const transformedMultiChoiceStream = await request.post(`${root}/v1/chat/completions`, {
    headers,
    data: chatRequest('guardrail-e2e-multi-choice-mask', true),
  })
  expect(transformedMultiChoiceStream.status()).toBe(200)
  const transformedMultiChoiceBody = await transformedMultiChoiceStream.text()
  expect(transformedMultiChoiceBody).toContain('[masked]')
  expect(transformedMultiChoiceBody).toContain('keep-choice')
  expect(transformedMultiChoiceBody).not.toContain('guardrail-e2e-mask')

  const oversizedStream = await request.post(`${root}/v1/chat/completions`, {
    headers,
    data: chatRequest('guardrail-e2e-oversize-stream', true),
  })
  expect(oversizedStream.status()).toBe(413)
  expect(oversizedStream.headers()['content-type']).toContain('application/json')
  expect((await oversizedStream.body()).byteLength).toBeLessThan(4 * 1024 * 1024)

  const streamDenied = await request.post(`${root}/v1/chat/completions`, {
    headers,
    data: chatRequest('guardrail-e2e-tool-call', true),
  })
  expect(streamDenied.status()).toBe(403)
  expect(streamDenied.headers()['content-type']).toContain('application/json')
  expect(await streamDenied.text()).not.toContain('rm -rf')
  for (const fixture of [
    'guardrail-e2e-parallel-tool-calls',
    'guardrail-e2e-malformed-tool-call',
  ]) {
    const denied = await request.post(`${root}/v1/chat/completions`, {
      headers,
      data: chatRequest(fixture, true),
    })
    expect(denied.status(), fixture).toBe(403)
    expect(denied.headers()['content-type']).toContain('application/json')
    const body = await denied.text()
    expect(body).not.toContain('rm -rf')
    expect(body).not.toContain('{not-json')
  }
})

test('keeps decision filters synchronized with browser navigation', async ({
  page,
  request,
  baseURL,
}) => {
  const root = baseURL ?? 'http://127.0.0.1:38080'
  await ensureAdminSession(page, request, root)
  await page.goto(`${root}/admin/observability/guardrails`)

  const evaluator = page.getByLabel('Evaluator')
  await evaluator.fill('deterministic')
  await page.getByRole('button', { name: 'Apply filters' }).click()
  await expect(page).toHaveURL(/evaluator=deterministic/)

  await evaluator.fill('managed')
  await page.getByRole('button', { name: 'Apply filters' }).click()
  await expect(page).toHaveURL(/evaluator=managed/)

  await page.goBack()
  await expect(page).toHaveURL(/evaluator=deterministic/)
  await expect(evaluator).toHaveValue('deterministic')
})

test('uses one deny path for direct and aggregate MCP calls before execution', async ({
  page,
  request,
  baseURL,
}) => {
  const root = baseURL ?? 'http://127.0.0.1:38080'
  const cookie = await ensureAdminSession(page, request, root)
  const adminHeaders = { cookie, 'content-type': 'application/json' }
  const serverKey = `notion-e2e-${Date.now()}`
  const createResponse = await request.post(`${root}/api/v1/admin/mcp/servers`, {
    headers: adminHeaders,
    data: {
      server_key: serverKey,
      display_name: 'Notion E2E',
      server_url: `${upstreamRoot}/mcp`,
      transport: 'streamable_http',
      auth_mode: 'none',
    },
  })
  expect(createResponse.status()).toBe(200)
  const created = (await createResponse.json()) as {
    data: { server: { id: string } }
  }
  const serverId = created.data.server.id

  const keysResponse = await request.get(`${root}/api/v1/admin/api-keys`, {
    headers: { cookie },
  })
  expect(keysResponse.status()).toBe(200)
  const keys = (await keysResponse.json()) as {
    data: {
      items: Array<{ name: string; owner_id: string; owner_kind: string }>
    }
  }
  const key = keys.data.items.find((item) => item.name === 'E2E Contract Key')
  expect(key).toBeDefined()
  const refreshResponse = await request.post(
    `${root}/api/v1/admin/mcp/servers/${serverId}/discovery-refresh`,
    { headers: adminHeaders },
  )
  expect(refreshResponse.status(), await refreshResponse.text()).toBe(200)
  const refresh = await refreshResponse.json()
  expect(refresh.data.status, JSON.stringify(refresh)).toBe('success')
  const catalogResponse = await request.get(`${root}/api/v1/admin/mcp/servers/${serverId}/tools`, {
    headers: { cookie },
  })
  expect(catalogResponse.status()).toBe(200)
  const catalog = (await catalogResponse.json()) as {
    data: { items: Array<{ id: string }> }
  }
  expect(catalog.data.items).toHaveLength(2)
  for (const tool of catalog.data.items) {
    const grantResponse = await request.put(`${root}/api/v1/admin/mcp/grants`, {
      headers: adminHeaders,
      data: {
        subject_kind: key!.owner_kind,
        subject_id: key!.owner_id,
        target_kind: 'tool',
        target_id: tool.id,
      },
    })
    expect(grantResponse.status(), await grantResponse.text()).toBe(200)
  }

  const mcpHeaders = {
    authorization: `Bearer ${gatewayApiKey}`,
    'content-type': 'application/json',
    accept: 'application/json, text/event-stream',
    'mcp-protocol-version': '2025-11-25',
  }
  const call = (name: string, arguments_: Record<string, unknown>, id: number | string) => ({
    jsonrpc: '2.0',
    id,
    method: 'tools/call',
    params: { name, arguments: arguments_ },
  })

  await request.delete(`${upstreamRoot}/__admin/mcp-executions`)
  const safeDirect = await request.post(`${root}/mcp/${serverKey}`, {
    headers: mcpHeaders,
    data: call('search', { query: 'safe' }, 1),
  })
  expect(safeDirect.status()).toBe(200)
  let executions = (await (await request.get(`${upstreamRoot}/__admin/mcp-executions`)).json()) as {
    executions: unknown[]
  }
  expect(executions.executions).toHaveLength(1)

  await request.delete(`${upstreamRoot}/__admin/mcp-executions`)
  const deniedDirect = await request.post(`${root}/mcp/${serverKey}`, {
    headers: mcpHeaders,
    data: call('delete_page', { page_id: 'page-e2e' }, 2),
  })
  expect(deniedDirect.status()).toBe(403)
  executions = (await (await request.get(`${upstreamRoot}/__admin/mcp-executions`)).json()) as {
    executions: unknown[]
  }
  expect(executions.executions).toHaveLength(0)

  const managedDeniedDirect = await request.post(`${root}/mcp/${serverKey}`, {
    headers: mcpHeaders,
    data: call('search', { query: 'guardrail-e2e-managed-deny' }, 3),
  })
  expect(managedDeniedDirect.status()).toBe(403)
  executions = (await (await request.get(`${upstreamRoot}/__admin/mcp-executions`)).json()) as {
    executions: unknown[]
  }
  expect(executions.executions).toHaveLength(0)

  const transformedResult = await request.post(`${root}/mcp/${serverKey}`, {
    headers: mcpHeaders,
    data: call('search', { query: 'guardrail-e2e-result-sensitive' }, 4),
  })
  expect(transformedResult.status()).toBe(200)
  const transformedPayload = await transformedResult.text()
  expect(transformedPayload).toContain('[masked]')
  expect(transformedPayload).not.toContain('guardrail-e2e-mask')

  const transformedSseResult = await request.post(`${root}/mcp/${serverKey}`, {
    headers: mcpHeaders,
    data: call('search', { query: 'guardrail-e2e-result-sensitive-sse' }, 'guardrail-e2e-mask'),
  })
  expect(transformedSseResult.status()).toBe(200)
  expect(transformedSseResult.headers()['content-type']).toContain('text/event-stream')
  const transformedSsePayload = await transformedSseResult.text()
  expect(transformedSsePayload).toContain('"id":"guardrail-e2e-mask"')
  expect(transformedSsePayload).toContain('[masked]')
  expect(transformedSsePayload.match(/guardrail-e2e-mask/g)).toHaveLength(1)

  await request.delete(`${upstreamRoot}/__admin/mcp-executions`)
  const deniedResult = await request.post(`${root}/mcp/${serverKey}`, {
    headers: mcpHeaders,
    data: call('search', { query: 'guardrail-e2e-result-deny' }, 5),
  })
  expect(deniedResult.status()).toBe(403)
  expect(await deniedResult.text()).not.toContain('guardrail-e2e-result-deny')
  executions = (await (await request.get(`${upstreamRoot}/__admin/mcp-executions`)).json()) as {
    executions: unknown[]
  }
  expect(executions.executions).toHaveLength(1)
  await request.delete(`${upstreamRoot}/__admin/mcp-executions`)

  const initializeResponse = await request.post(`${root}/mcp`, {
    headers: mcpHeaders,
    data: {
      jsonrpc: '2.0',
      id: 3,
      method: 'initialize',
      params: {
        protocolVersion: '2025-11-25',
        capabilities: {},
        clientInfo: { name: 'guardrail-e2e', version: '1.0.0' },
      },
    },
  })
  expect(initializeResponse.status()).toBe(200)
  const sessionId = initializeResponse.headers()['mcp-session-id']
  expect(sessionId).toBeTruthy()
  const aggregateHeaders = { ...mcpHeaders, 'mcp-session-id': sessionId }
  const initializedResponse = await request.post(`${root}/mcp`, {
    headers: aggregateHeaders,
    data: { jsonrpc: '2.0', method: 'notifications/initialized' },
  })
  expect(initializedResponse.status()).toBe(202)

  const toolsResponse = await request.post(`${root}/mcp`, {
    headers: aggregateHeaders,
    data: { jsonrpc: '2.0', id: 4, method: 'tools/list', params: {} },
  })
  expect(toolsResponse.status(), await toolsResponse.text()).toBe(200)
  const toolsPayload = await toolsResponse.json()
  const aggregateCallTool = toolsPayload.result.tools.find(
    (tool: { name: string }) => tool.name === 'call_tool',
  )
  expect(aggregateCallTool, JSON.stringify(toolsPayload)).toBeDefined()

  const transformedAggregate = await request.post(`${root}/mcp`, {
    headers: aggregateHeaders,
    data: call(
      aggregateCallTool.name,
      {
        address: `mcp://${serverKey}/tools/search`,
        arguments: { query: 'guardrail-e2e-result-sensitive' },
      },
      5,
    ),
  })
  expect(transformedAggregate.status()).toBe(200)
  const transformedAggregatePayload = await transformedAggregate.text()
  expect(transformedAggregatePayload).toContain('[masked]')
  expect(transformedAggregatePayload).not.toContain('guardrail-e2e-mask')
  await request.delete(`${upstreamRoot}/__admin/mcp-executions`)

  const deniedAggregate = await request.post(`${root}/mcp`, {
    headers: aggregateHeaders,
    data: call(
      aggregateCallTool.name,
      {
        address: `mcp://${serverKey}/tools/delete_page`,
        arguments: { page_id: 'page-e2e' },
      },
      6,
    ),
  })
  expect(deniedAggregate.status()).toBe(200)
  const aggregatePayload = await deniedAggregate.json()
  expect(JSON.stringify(aggregatePayload), JSON.stringify(aggregatePayload)).toContain(
    'guardrail_policy_denied',
  )
  executions = (await (await request.get(`${upstreamRoot}/__admin/mcp-executions`)).json()) as {
    executions: unknown[]
  }
  expect(executions.executions).toHaveLength(0)
})

test('denies destructive shell execution and exposes only privacy-safe decision data', async ({
  page,
  request,
  baseURL,
}) => {
  const root = baseURL ?? 'http://127.0.0.1:38080'
  const command = 'rm -rf /tmp/oceans-guardrail-e2e'
  const response = await request.post(`${root}/api/v1/guardrails/evaluate`, {
    headers: {
      authorization: `Bearer ${gatewayApiKey}`,
      'content-type': 'application/json',
    },
    data: {
      tool_name: 'bash',
      command,
    },
  })

  expect(response.status()).toBe(200)
  const decision = (await response.json()) as {
    decision_id: string
    action: string
    allowed: boolean
    reason_code: string | null
  }
  expect(decision).toMatchObject({
    action: 'deny',
    allowed: false,
    reason_code: 'filesystem.recursive_force_remove',
  })

  const failOpenResponse = await request.post(`${root}/api/v1/guardrails/evaluate`, {
    headers: {
      authorization: `Bearer ${gatewayApiKey}`,
      'content-type': 'application/json',
    },
    data: {
      tool_name: 'bash',
      command: 'echo guardrail-e2e-fail-open',
    },
  })
  expect(failOpenResponse.status()).toBe(200)
  const failOpenDecision = (await failOpenResponse.json()) as {
    decision_id: string
    action: string
    allowed: boolean
    failure_disposition: string | null
  }
  expect(failOpenDecision).toMatchObject({
    action: 'audit',
    allowed: true,
    failure_disposition: 'fail_open',
  })

  const cookie = await ensureAdminSession(page, request, root)
  const eventsResponse = await request.get(`${root}/api/v1/admin/guardrails/decisions`, {
    headers: { cookie },
  })
  expect(eventsResponse.status()).toBe(200)
  const events = await eventsResponse.json()
  expect(JSON.stringify(events)).toContain(decision.decision_id)
  expect(JSON.stringify(events)).toContain(failOpenDecision.decision_id)
  expect(JSON.stringify(events)).toContain('fail_open')
  expect(JSON.stringify(events)).not.toContain(command)

  await page.goto(`${root}/admin/observability/guardrails`)
  await expect(page.getByText('filesystem.recursive_force_remove').first()).toBeVisible()
  await expect(page.getByText(command)).toHaveCount(0)
  await expect(page.getByRole('button', { name: /save|edit|update/i })).toHaveCount(0)
})
