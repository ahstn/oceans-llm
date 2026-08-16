import { expect, test } from 'playwright/test'

import { ensureAdminSession } from './admin-session'
import { requireEnv } from './env'

const gatewayApiKey = process.env.E2E_GATEWAY_API_KEY ?? 'gwk_e2e.secret-value'

test('correlates a live agent request and exposes it through the admin session explorer', async ({
  request,
  page,
}) => {
  const root = requireEnv('E2E_BASE_URL')
  const adminCookie = await ensureAdminSession(page, request, root)
  const externalSessionId = `e2e-agent-session-${Date.now()}`
  const startedAfter = new Date(Date.now() - 1_000).toISOString()

  const completionResponse = await request.post(`${root}/v1/chat/completions`, {
    headers: {
      authorization: `Bearer ${gatewayApiKey}`,
      'content-type': 'application/json',
      'idempotency-key': externalSessionId,
      'user-agent': 'opencode/1.0',
      'x-session-id': externalSessionId,
    },
    data: {
      model: 'fast',
      messages: [{ role: 'user', content: 'inspect the session analysis contract' }],
      tools: [
        {
          type: 'function',
          function: {
            name: 'read',
            description: 'Read a bounded file range',
            parameters: { type: 'object', properties: {} },
          },
        },
      ],
    },
  })
  expect(completionResponse.status()).toBe(200)

  const listResponse = await request.get(
    `${root}/api/v1/admin/observability/agent-sessions?harness_key=opencode&requested_model_key=fast&started_after=${encodeURIComponent(startedAfter)}`,
    { headers: { cookie: adminCookie } },
  )
  expect(listResponse.status()).toBe(200)
  const listBody = (await listResponse.json()) as {
    data: {
      total: number
      items: Array<{
        session_id: string
        session_source_hash: string | null
        harness_key: string | null
        requested_model_key: string
        lifecycle: string
        request_count: number
        efficiency_score: number | null
      }>
    }
  }
  expect(listBody.data.total).toBe(1)
  const [session] = listBody.data.items
  expect(session.session_source_hash).toMatch(/^sha256:[0-9a-f]{64}$/)
  expect(session.session_source_hash).not.toBe(externalSessionId)
  expect(session).toMatchObject({
    harness_key: 'opencode',
    requested_model_key: 'fast',
    lifecycle: 'open',
    request_count: 1,
    efficiency_score: null,
  })

  const detailResponse = await request.get(
    `${root}/api/v1/admin/observability/agent-sessions/${session.session_id}`,
    { headers: { cookie: adminCookie } },
  )
  expect(detailResponse.status()).toBe(200)
  const detailBody = (await detailResponse.json()) as {
    data: {
      session: { session_id: string; session_source_hash: string | null }
      requests: Array<{ request_id: string }>
      observations: unknown[]
      report: unknown | null
    }
  }
  expect(detailBody.data.session).toMatchObject({
    session_id: session.session_id,
    session_source_hash: session.session_source_hash,
  })
  expect(detailBody.data.requests).toHaveLength(1)
  expect(detailBody.data.report).toBeNull()

  await page.goto(
    `/admin/observability/agent-sessions?session_source_hash=${encodeURIComponent(session.session_source_hash ?? '')}`,
  )
  await expect(page.getByRole('heading', { name: 'Agent sessions' })).toBeVisible()
  await expect(page.getByText('fast').first()).toBeVisible()
  await expect(page.getByText('Open').first()).toBeVisible()
  await page.getByText('fast').first().click()
  await expect(page.getByText('Agent session details')).toBeVisible()
  await expect(page.getByText(externalSessionId)).toHaveCount(0)
  await expect(page.getByText('Calibration data')).toBeVisible()
})
