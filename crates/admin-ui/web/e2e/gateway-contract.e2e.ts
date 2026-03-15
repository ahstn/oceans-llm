import { expect, test } from 'playwright/test'

import { requireEnv, stubAdminUrl } from './env'

const gatewayApiKey = process.env.E2E_GATEWAY_API_KEY ?? 'gwk_e2e.secret-value'

test('gateway exposes the seeded model and forwards chat completions to the stub upstream', async ({
  request,
  baseURL,
}) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')

  const modelsResponse = await request.get(`${root}/v1/models`, {
    headers: {
      authorization: `Bearer ${gatewayApiKey}`,
    },
  })

  expect(modelsResponse.ok()).toBe(true)
  expect(await modelsResponse.json()).toEqual({
    object: 'list',
    data: [
      {
        id: 'fast',
        object: 'model',
        created: 0,
        owned_by: 'gateway',
      },
    ],
  })

  const clearResponse = await request.delete(stubAdminUrl('/__admin/requests'))
  expect(clearResponse.ok()).toBe(true)

  const completionResponse = await request.post(`${root}/v1/chat/completions`, {
    headers: {
      authorization: `Bearer ${gatewayApiKey}`,
      'content-type': 'application/json',
    },
    data: {
      model: 'fast',
      messages: [{ role: 'user', content: 'ping' }],
    },
  })

  expect(completionResponse.status()).toBe(200)
  expect(completionResponse.headers()['x-request-id']).toBeTruthy()
  expect(await completionResponse.json()).toEqual({
    id: 'chatcmpl-e2e-1',
    object: 'chat.completion',
    created: 1_741_510_000,
    model: 'fast',
    choices: [
      {
        index: 0,
        message: {
          role: 'assistant',
          content: 'pong',
        },
        finish_reason: 'stop',
      },
    ],
    usage: {
      prompt_tokens: 8,
      completion_tokens: 4,
      total_tokens: 12,
    },
  })

  const capturedResponse = await request.get(stubAdminUrl('/__admin/requests'))
  expect(capturedResponse.ok()).toBe(true)

  const capturedPayload = (await capturedResponse.json()) as {
    requests: Array<{
      method: string
      path: string
      headers: Record<string, string>
      body: {
        model: string
        messages: Array<{ role: string; content: string }>
      }
    }>
  }

  expect(capturedPayload.requests).toHaveLength(1)

  const [captured] = capturedPayload.requests
  expect(captured.method).toBe('POST')
  expect(captured.path).toBe('/v1/chat/completions')
  expect(captured.headers.authorization).toBe('Bearer upstream-e2e-token')
  expect(captured.body.model).toBe('gpt-4o-mini')
  expect(captured.body.messages).toEqual([
    expect.objectContaining({ role: 'user', content: 'ping' }),
  ])
})
