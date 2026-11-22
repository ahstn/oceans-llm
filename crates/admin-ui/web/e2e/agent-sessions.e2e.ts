import { createHash } from 'node:crypto'
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
  const serviceAccountsResponse = await request.get(
    `${root}/api/v1/admin/identity/service-accounts`,
    { headers: { cookie: adminCookie } },
  )
  expect(serviceAccountsResponse.status()).toBe(200)
  const serviceAccountsBody = (await serviceAccountsResponse.json()) as {
    data: { service_accounts: Array<{ id: string; name: string }> }
  }
  const serviceAccount = serviceAccountsBody.data.service_accounts.find(
    (account) => account.name === 'E2E Seed API Keys',
  )
  expect(serviceAccount).toBeDefined()
  const externalSessionId = `e2e-agent-session-${Date.now()}`
  const startedAfter = new Date(Date.now() - 1_000).toISOString()
  const sessionSourceHash = `sha256:${createHash('sha256')
    .update(`service_account:${serviceAccount?.id ?? ''}`)
    .update('\0opencode\0lineage-v1\0')
    .update(externalSessionId)
    .digest('hex')}`

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

  const listUrl = `${root}/api/v1/admin/observability/agent-sessions?harness_key=opencode&requested_model_key=fast&service_account_id=${encodeURIComponent(serviceAccount?.id ?? '')}&session_source_hash=${encodeURIComponent(sessionSourceHash)}&started_after=${encodeURIComponent(startedAfter)}`
  type AgentSessionList = {
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
  // Passive correlation runs after the response, and the gateway writes the
  // session row before it links the request, so wait for the request count too.
  let listBody: AgentSessionList | undefined
  await expect
    .poll(async () => {
      const response = await request.get(listUrl, { headers: { cookie: adminCookie } })
      if (response.status() !== 200) return { status: response.status() }
      listBody = (await response.json()) as AgentSessionList
      return {
        total: listBody.data.total,
        request_counts: listBody.data.items.map((item) => item.request_count),
      }
    })
    .toEqual({ total: 1, request_counts: [1] })

  if (!listBody) throw new Error('agent session list was never read')
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
      report: { score: number | null } | null
    }
  }
  expect(detailBody.data.session).toMatchObject({
    session_id: session.session_id,
    session_source_hash: session.session_source_hash,
  })
  expect(detailBody.data.requests).toHaveLength(1)
  // The background analyzer also reports on open sessions every few seconds, so
  // a report may already exist. During calibration it never carries a score.
  expect(detailBody.data.report?.score ?? null).toBeNull()

  await page.goto(
    `/admin/observability/agent-sessions?session_source_hash=${encodeURIComponent(session.session_source_hash ?? '')}`,
  )
  await expect(page.getByRole('heading', { name: 'Agent sessions' })).toBeVisible()
  // The URL filter shows as a chip, and the desktop table (not the hidden card list) shows the row.
  await expect(page.getByTestId('agent-session-filter-session-source-hash')).toHaveValue(
    session.session_source_hash ?? '',
  )
  const table = page.getByTestId('agent-session-table')
  await expect(table.getByText('fast').first()).toBeVisible()
  await expect(table.getByText('Open').first()).toBeVisible()
  await table.getByText('fast').first().click()
  await expect(page.getByText('Agent session details')).toBeVisible()
  await expect(page.getByText(externalSessionId)).toHaveCount(0)
  await expect(page.getByText('Calibration data')).toBeVisible()
})
