import assert from 'node:assert/strict'
import { generateKeyPairSync } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'

import {
  createAppJwt,
  parseSse,
  resultStatus,
  runCanary,
  sanitizeEvidence,
  selectModel,
  summarizeBilling,
  validateStreamResult,
} from './copilot-installation-canary.mjs'

const compatibilityProfile = JSON.parse(
  readFileSync(new URL('./copilot-compatibility-profile.json', import.meta.url), 'utf8'),
)

test('sanitizes GitHub and Bearer tokens from report evidence', () => {
  const evidence = sanitizeEvidence(
    'ghs_secret github_pat_secret Bearer opaque-token and ghu_user-token',
  )

  assert.equal(evidence.includes('secret'), false)
  assert.equal(evidence.includes('opaque-token'), false)
  assert.match(evidence, /\[REDACTED_GITHUB_TOKEN\]/u)
  assert.match(evidence, /Bearer \[REDACTED\]/u)
})

test('creates an RS256 GitHub App JWT without exposing the private key', () => {
  const { privateKey } = generateKeyPairSync('rsa', { modulusLength: 2048 })
  const jwt = createAppJwt(123, privateKey.export({ type: 'pkcs1', format: 'pem' }), 1_000)
  const [header, claims, signature] = jwt.split('.')

  assert.deepEqual(JSON.parse(Buffer.from(header, 'base64url')), { alg: 'RS256', typ: 'JWT' })
  assert.deepEqual(JSON.parse(Buffer.from(claims, 'base64url')), {
    iat: 940,
    exp: 1_540,
    iss: 123,
  })
  assert.ok(signature.length > 100)
})

test('selects only an enabled model with the requested endpoint and features', () => {
  const models = [
    {
      id: 'gpt-canary',
      supported_endpoints: ['/v1/chat/completions'],
      capabilities: { supports: { streaming: true, tool_calls: true } },
    },
  ]

  assert.equal(
    selectModel(models, 'gpt-canary', 'chat/completions', { streaming: true, tools: true })
      .id,
    'gpt-canary',
  )
  assert.throws(
    () => selectModel(models, 'gpt-canary', 'v1/messages'),
    /does not advertise/u,
  )
})

test('parses OpenAI and Anthropic SSE records', () => {
  const parsed = parseSse(
    'data: {"choices":[{"delta":{"content":"ok"}}]}\n\n' +
      'event: message_stop\ndata: {"type":"message_stop"}\n\n' +
      'data: [DONE]\n\n',
  )

  assert.equal(parsed.events.length, 2)
  assert.equal(parsed.events[1].type, 'message_stop')
  assert.equal(parsed.done, true)
})

test('rejects error-only and empty successful streams', () => {
  assert.throws(
    () =>
      validateStreamResult(
        { events: [{ error: { message: 'upstream failed' } }], done: true },
        'chat/completions',
        'chat',
      ),
    /error event/u,
  )
  assert.throws(
    () =>
      validateStreamResult(
        { events: [{ type: 'message_stop' }], done: false },
        'v1/messages',
        'messages',
      ),
    /no assistant content delta/u,
  )
})

test('summarizes the public billing aggregate without copying response details', () => {
  assert.deepEqual(
    summarizeBilling({
      usageItems: [
        { netQuantity: 1, netAmount: 2.5 },
        { netQuantity: '3', netAmount: '4.5' },
      ],
    }),
    { items: 2, netQuantity: 4, netAmount: 7 },
  )
})

test('marks required failures and unavailable checks distinctly', () => {
  assert.equal(resultStatus([{ required: true, status: 'PASS' }]), 'PASS')
  assert.equal(resultStatus([{ required: true, status: 'UNAVAILABLE' }]), 'INCOMPLETE')
  assert.equal(resultStatus([{ required: true, status: 'FAIL' }]), 'FAIL')
  assert.equal(resultStatus([{ required: false, status: 'UNAVAILABLE' }]), 'PASS')
})

test('runs the complete mocked canary without writing tokens to its report', async () => {
  const temporaryDirectory = await mkdtemp(join(tmpdir(), 'oceans-copilot-canary-'))
  const privateKeyPath = join(temporaryDirectory, 'app.pem')
  const { privateKey } = generateKeyPairSync('rsa', { modulusLength: 2048 })
  await writeFile(privateKeyPath, privateKey.export({ type: 'pkcs1', format: 'pem' }), {
    mode: 0o600,
  })

  const requests = []
  let mintCount = 0
  let billingCount = 0
  const jsonResponse = (body, init = {}) =>
    new Response(JSON.stringify(body), {
      status: init.status || 200,
      headers: { 'content-type': 'application/json', ...init.headers },
    })
  const fetchImplementation = async (url, options = {}) => {
    const parsed = new URL(url)
    const method = options.method || 'GET'
    const headers = new Headers(options.headers)
    const body = options.body ? JSON.parse(options.body) : null
    requests.push({ path: parsed.pathname, method, headers, body })

    if (parsed.pathname === '/app/installations/456' && method === 'GET') {
      return jsonResponse({
        account: { login: 'example-org', type: 'Organization' },
        repository_selection: 'all',
        permissions: { copilot_requests: 'write' },
      })
    }
    if (parsed.pathname === '/app/installations/456/access_tokens' && method === 'POST') {
      mintCount += 1
      return jsonResponse({
        token: `ghs_canary_secret_${mintCount}`,
        expires_at: new Date(Date.now() + 60 * 60 * 1_000).toISOString(),
        permissions: { copilot_requests: 'write' },
        repositories: [{ id: 789 }],
      })
    }
    if (parsed.pathname === '/installation/token' && method === 'DELETE') {
      return new Response(null, { status: 204 })
    }
    if (parsed.pathname.includes('/settings/billing/ai_credit/usage')) {
      billingCount += 1
      return jsonResponse({
        organization: 'example-org',
        usageItems: [{ netQuantity: billingCount === 1 ? 1 : 2, netAmount: billingCount }],
      })
    }
    if (parsed.pathname === '/models') {
      return jsonResponse({
        data: [
          {
            id: 'gpt-canary',
            supported_endpoints: ['/chat/completions'],
            capabilities: {
              supports: {
                streaming: true,
                tool_calls: true,
                vision: true,
                structured_outputs: false,
              },
            },
          },
          {
            id: 'claude-canary',
            supported_endpoints: ['/v1/messages'],
            capabilities: { supports: { streaming: true } },
          },
        ],
      })
    }
    if (parsed.pathname === '/chat/completions' && body?.stream) {
      return new Response('data: {"choices":[{"delta":{"content":"ok"}}]}\n\ndata: [DONE]\n\n', {
        headers: { 'content-type': 'text/event-stream' },
      })
    }
    if (parsed.pathname === '/chat/completions' && body?.tools) {
      return jsonResponse({
        choices: [
          {
            message: {
              role: 'assistant',
              content: null,
              tool_calls: [
                {
                  id: 'call_canary',
                  type: 'function',
                  function: { name: 'get_canary_value', arguments: '{}' },
                },
              ],
            },
          },
        ],
      })
    }
    if (parsed.pathname === '/chat/completions') {
      return jsonResponse({ choices: [{ message: { role: 'assistant', content: 'CANARY_OK' } }] })
    }
    if (parsed.pathname === '/v1/messages' && body?.stream) {
      return new Response(
        'event: message_start\ndata: {"type":"message_start"}\n\n' +
          'event: content_block_delta\ndata: {"type":"content_block_delta","delta":{"type":"text_delta","text":"ok"}}\n\n' +
          'event: message_stop\ndata: {"type":"message_stop"}\n\n',
        { headers: { 'content-type': 'text/event-stream' } },
      )
    }
    if (parsed.pathname === '/v1/messages') {
      return jsonResponse({ content: [{ type: 'text', text: 'CANARY_OK' }] })
    }
    return jsonResponse({ error: 'not found' }, { status: 404 })
  }

  let output = ''
  const environment = {
    COPILOT_CANARY_APP_ID: '123',
    COPILOT_CANARY_INSTALLATION_ID: '456',
    COPILOT_CANARY_REPOSITORY_ID: '789',
    COPILOT_CANARY_EXPECTED_OWNER: 'example-org',
    COPILOT_CANARY_PRIVATE_KEY_PATH: privateKeyPath,
    COPILOT_CANARY_CHAT_MODEL: 'gpt-canary',
    COPILOT_CANARY_MESSAGES_MODEL: 'claude-canary',
    COPILOT_CANARY_BILLING_TOKEN: 'github_pat_billing_secret',
    COPILOT_CANARY_GITHUB_API_URL: 'https://github.test',
    COPILOT_CANARY_API_URL: 'https://copilot.test',
  }

  try {
    const exitCode = await runCanary(environment, fetchImplementation, {
      write(chunk) {
        output += chunk
      },
    })
    const report = JSON.parse(output)
    assert.equal(exitCode, 0, output)
    assert.equal(report.result, 'PASS')
    assert.equal(output.includes('ghs_canary_secret'), false)
    assert.equal(output.includes('github_pat_billing_secret'), false)
    const modelEvidence = report.checks.find((check) => check.name === 'models').evidence
    assert.match(
      modelEvidence,
      /"supports":\{"streaming":true,"tool_calls":true,"vision":true,"structured_outputs":false\}/,
    )
    assert.equal(
      report.checks.find((check) => check.name === 'billing_usage_delta').status,
      'PASS',
    )
    assert.equal(
      requests.some(
        (request) =>
          request.path === '/chat/completions' &&
          request.headers.get('x-initiator') === 'agent' &&
          request.body?.messages?.at(-1)?.role === 'tool',
      ),
      true,
    )
    assert.equal(
      requests
        .filter((request) => request.path === '/v1/messages')
        .every(
          (request) =>
            request.headers.get('anthropic-version') === compatibilityProfile.anthropic_version,
        ),
      true,
    )
    assert.equal(
      requests
        .filter((request) => request.path === '/installation/token')
        .every((request) => request.method === 'DELETE'),
      true,
    )
  } finally {
    await rm(temporaryDirectory, { recursive: true, force: true })
  }
})

test('revokes a minted token when scope validation fails', async () => {
  const temporaryDirectory = await mkdtemp(join(tmpdir(), 'oceans-copilot-canary-cleanup-'))
  const privateKeyPath = join(temporaryDirectory, 'app.pem')
  const { privateKey } = generateKeyPairSync('rsa', { modulusLength: 2048 })
  await writeFile(privateKeyPath, privateKey.export({ type: 'pkcs1', format: 'pem' }), {
    mode: 0o600,
  })

  const revokedTokens = []
  const fetchImplementation = async (url, options = {}) => {
    const path = new URL(url).pathname
    const method = options.method || 'GET'
    if (path === '/app/installations/456' && method === 'GET') {
      return new Response(
        JSON.stringify({
          account: { login: 'example-org', type: 'Organization' },
          repository_selection: 'all',
          permissions: { copilot_requests: 'write' },
        }),
        { headers: { 'content-type': 'application/json' } },
      )
    }
    if (path === '/app/installations/456/access_tokens' && method === 'POST') {
      return new Response(
        JSON.stringify({
          token: 'ghs_invalid_scope_secret',
          expires_at: new Date(Date.now() + 60 * 60 * 1_000).toISOString(),
          permissions: { copilot_requests: 'read' },
          repositories: [{ id: 789 }],
        }),
        { headers: { 'content-type': 'application/json' } },
      )
    }
    if (path === '/installation/token' && method === 'DELETE') {
      revokedTokens.push(new Headers(options.headers).get('authorization'))
      return new Response(null, { status: 204 })
    }
    throw new Error(`Unexpected request: ${method} ${path}`)
  }

  const environment = {
    COPILOT_CANARY_APP_ID: '123',
    COPILOT_CANARY_INSTALLATION_ID: '456',
    COPILOT_CANARY_REPOSITORY_ID: '789',
    COPILOT_CANARY_EXPECTED_OWNER: 'example-org',
    COPILOT_CANARY_PRIVATE_KEY_PATH: privateKeyPath,
    COPILOT_CANARY_GITHUB_API_URL: 'https://github.test',
    COPILOT_CANARY_API_URL: 'https://copilot.test',
  }

  try {
    let output = ''
    const exitCode = await runCanary(environment, fetchImplementation, {
      write(chunk) {
        output += chunk
      },
    })

    assert.equal(exitCode, 1, output)
    assert.deepEqual(revokedTokens, ['Bearer ghs_invalid_scope_secret'])
    assert.equal(output.includes('ghs_invalid_scope_secret'), false)
  } finally {
    await rm(temporaryDirectory, { recursive: true, force: true })
  }
})
