import { expect, test } from 'playwright/test'

import { ensureAdminSession } from './admin-session'
import { requireEnv } from './env'

const gatewayApiKey = process.env.E2E_GATEWAY_API_KEY ?? 'gwk_e2e.secret-value'

test('correlates a live agent request and exposes it through the admin task explorer', async ({
  request,
  page,
  baseURL,
}) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  const adminCookie = await ensureAdminSession(page, request, root)
  const externalSessionId = `e2e-agent-session-${Date.now()}`

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
      messages: [{ role: 'user', content: 'inspect the task analysis contract' }],
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
    `${root}/api/v1/admin/observability/agent-tasks?external_session_id=${encodeURIComponent(externalSessionId)}`,
    { headers: { cookie: adminCookie } },
  )
  expect(listResponse.status()).toBe(200)
  const listBody = (await listResponse.json()) as {
    data: {
      total: number
      items: Array<{
        task_id: string
        external_session_id: string | null
        harness_key: string | null
        requested_model_key: string
        lifecycle: string
        request_count: number
        efficiency_score: number | null
      }>
    }
  }
  expect(listBody.data.total).toBe(1)
  const [task] = listBody.data.items
  expect(task).toMatchObject({
    external_session_id: externalSessionId,
    harness_key: 'opencode',
    requested_model_key: 'fast',
    lifecycle: 'open',
    request_count: 1,
    efficiency_score: null,
  })

  const detailResponse = await request.get(
    `${root}/api/v1/admin/observability/agent-tasks/${task.task_id}`,
    { headers: { cookie: adminCookie } },
  )
  expect(detailResponse.status()).toBe(200)
  const detailBody = (await detailResponse.json()) as {
    data: {
      task: { task_id: string; external_session_id: string | null }
      requests: Array<{ request_id: string }>
      observations: unknown[]
      report: unknown | null
    }
  }
  expect(detailBody.data.task).toMatchObject({
    task_id: task.task_id,
    external_session_id: externalSessionId,
  })
  expect(detailBody.data.requests).toHaveLength(1)
  expect(detailBody.data.report).toBeNull()

  await page.goto(
    `/admin/observability/agent-tasks?external_session_id=${encodeURIComponent(externalSessionId)}`,
  )
  await expect(page.getByRole('heading', { name: 'Agent tasks' })).toBeVisible()
  await expect(page.getByText('fast').first()).toBeVisible()
  await expect(page.getByText('Open').first()).toBeVisible()
  await page.getByText('fast').first().click()
  await expect(page.getByText('Task diagnostics')).toBeVisible()
  await expect(page.getByText(externalSessionId)).toBeVisible()
  await expect(page.getByText('Shadow diagnostics')).toBeVisible()
})
