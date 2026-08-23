import { createServer } from 'node:http'

const port = Number(process.env.E2E_UPSTREAM_PORT ?? '18081')
const requests = []
const mcpExecutions = []

function readJson(req) {
  return new Promise((resolve, reject) => {
    let body = ''
    req.setEncoding('utf8')
    req.on('data', (chunk) => {
      body += chunk
    })
    req.on('end', () => {
      if (!body) {
        resolve(null)
        return
      }

      try {
        resolve(JSON.parse(body))
      } catch (error) {
        reject(error)
      }
    })
    req.on('error', reject)
  })
}

function maskStructuredText(text) {
  try {
    const mask = (value) => {
      if (typeof value === 'string') {
        return value.includes('guardrail-e2e-mask') ? '[masked]' : value
      }
      if (Array.isArray(value)) {
        return value.map(mask)
      }
      if (value && typeof value === 'object') {
        return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, mask(item)]))
      }
      return value
    }
    return JSON.stringify(mask(JSON.parse(text)))
  } catch {
    return '[masked]'
  }
}

function sendJson(res, statusCode, payload) {
  res.writeHead(statusCode, { 'content-type': 'application/json' })
  res.end(JSON.stringify(payload))
}

function collectHeaders(headers) {
  return Object.fromEntries(
    Object.entries(headers).flatMap(([key, value]) => {
      if (typeof value === 'undefined') {
        return []
      }

      if (Array.isArray(value)) {
        return [[key, value.join(', ')]]
      }

      return [[key, value]]
    }),
  )
}

const server = createServer(async (req, res) => {
  const url = new URL(req.url ?? '/', `http://${req.headers.host ?? '127.0.0.1'}`)

  if (req.method === 'GET' && url.pathname === '/__admin/requests') {
    sendJson(res, 200, { requests })
    return
  }

  if (req.method === 'DELETE' && url.pathname === '/__admin/requests') {
    requests.length = 0
    sendJson(res, 200, { requests })
    return
  }

  if (req.method === 'GET' && url.pathname === '/__admin/mcp-executions') {
    sendJson(res, 200, { executions: mcpExecutions })
    return
  }

  if (req.method === 'DELETE' && url.pathname === '/__admin/mcp-executions') {
    mcpExecutions.length = 0
    sendJson(res, 200, { executions: mcpExecutions })
    return
  }

  if (req.method === 'POST' && url.pathname === '/mcp') {
    const body = await readJson(req)
    if (body?.method === 'initialize') {
      sendJson(res, 200, {
        jsonrpc: '2.0',
        id: body.id,
        result: {
          protocolVersion: '2025-11-25',
          capabilities: { tools: {} },
          serverInfo: { name: 'notion-e2e', version: '1.0.0' },
        },
      })
      return
    }
    if (body?.method === 'notifications/initialized') {
      res.writeHead(202)
      res.end()
      return
    }
    if (body?.method === 'tools/list') {
      sendJson(res, 200, {
        jsonrpc: '2.0',
        id: body.id,
        result: {
          tools: [
            {
              name: 'delete_page',
              description: 'Delete a Notion page',
              inputSchema: {
                type: 'object',
                properties: { page_id: { type: 'string' } },
                required: ['page_id'],
              },
            },
            {
              name: 'search',
              description: 'Search Notion',
              inputSchema: {
                type: 'object',
                properties: { query: { type: 'string' } },
              },
            },
          ],
        },
      })
      return
    }
    if (body?.method === 'tools/call') {
      mcpExecutions.push(body.params)
      if (body.params?.arguments?.query === 'guardrail-e2e-result-sensitive-sse') {
        res.writeHead(200, { 'content-type': 'text/event-stream' })
        res.end(`data: ${JSON.stringify({
          jsonrpc: '2.0',
          id: body.id,
          result: {
            content: [{ type: 'text', text: 'guardrail-e2e-mask' }],
            isError: false,
          },
        })}\n\n`)
        return
      }
      const resultText = body.params?.arguments?.query === 'guardrail-e2e-result-sensitive'
        ? 'guardrail-e2e-mask'
        : body.params?.arguments?.query === 'guardrail-e2e-result-deny'
          ? 'guardrail-e2e-managed-deny'
          : 'upstream executed'
      sendJson(res, 200, {
        jsonrpc: '2.0',
        id: body.id,
        result: {
          content: [{ type: 'text', text: resultText }],
          isError: false,
        },
      })
      return
    }
    sendJson(res, 200, { jsonrpc: '2.0', id: body?.id ?? null, result: {} })
    return
  }

  if (
    req.method === 'POST'
    && url.pathname.startsWith('/modelarmor/')
    && (url.pathname.endsWith(':sanitizeUserPrompt')
      || url.pathname.endsWith(':sanitizeModelResponse'))
  ) {
    const body = await readJson(req)
    const text = body?.userPromptData?.text ?? body?.modelResponseData?.text ?? ''
    if (text.includes('guardrail-e2e-fail-open')) {
      sendJson(res, 503, { error: { message: 'managed fixture unavailable' } })
      return
    }
    const matched = text.includes('guardrail-e2e-managed-deny')
    const masked = text.includes('guardrail-e2e-mask')
    const maskedText = maskStructuredText(text)
    sendJson(res, 200, {
      sanitizationResult: {
        invocationResult: 'SUCCESS',
        filterMatchState: matched || masked ? 'MATCH_FOUND' : 'NO_MATCH_FOUND',
        ...(masked
          ? {
              filterResults: {
                sdp: {
                  sdpFilterResult: {
                    deidentifyResult: { data: { text: maskedText } },
                  },
                },
              },
            }
          : {}),
      },
    })
    return
  }

  if (req.method === 'POST' && url.pathname === '/v1/chat/completions') {
    const body = await readJson(req)
    requests.push({
      method: req.method,
      path: url.pathname,
      headers: collectHeaders(req.headers),
      body,
    })

    if (body?.stream) {
      res.writeHead(200, { 'content-type': 'text/event-stream' })
      if (body?.user === 'guardrail-e2e-tool-call') {
        res.write(
          `data: ${JSON.stringify({
            id: 'chatcmpl-e2e-stream',
            object: 'chat.completion.chunk',
            choices: [{
              index: 0,
              delta: {
                tool_calls: [{
                  index: 0,
                  id: 'call-e2e',
                  type: 'function',
                  function: { name: 'bash', arguments: '{"command":"rm ' },
                }],
              },
            }],
          })}\n\n`,
        )
        res.write(
          `data: ${JSON.stringify({
            id: 'chatcmpl-e2e-stream',
            object: 'chat.completion.chunk',
            choices: [{
              index: 0,
              delta: {
                tool_calls: [{
                  index: 0,
                  function: { arguments: '-rf /tmp/oceans-stream-e2e"}' },
                }],
              },
              finish_reason: 'tool_calls',
            }],
          })}\n\n`,
        )
      } else if (body?.user === 'guardrail-e2e-parallel-tool-calls') {
        res.write(
          `data: ${JSON.stringify({
            id: 'chatcmpl-e2e-parallel',
            object: 'chat.completion.chunk',
            choices: [{
              index: 0,
              delta: {
                tool_calls: [
                  {
                    index: 0,
                    id: 'call-safe',
                    type: 'function',
                    function: { name: 'bash', arguments: '{"command":"printf ' },
                  },
                  {
                    index: 1,
                    id: 'call-denied',
                    type: 'function',
                    function: { name: 'bash', arguments: '{"command":"rm ' },
                  },
                ],
              },
            }],
          })}\n\n`,
        )
        res.write(
          `data: ${JSON.stringify({
            id: 'chatcmpl-e2e-parallel',
            object: 'chat.completion.chunk',
            choices: [{
              index: 0,
              delta: {
                tool_calls: [
                  { index: 0, function: { arguments: 'safe"}' } },
                  { index: 1, function: { arguments: '-rf /tmp/oceans-mixed-e2e"}' } },
                ],
              },
              finish_reason: 'tool_calls',
            }],
          })}\n\n`,
        )
      } else if (body?.user === 'guardrail-e2e-malformed-tool-call') {
        res.write(
          `data: ${JSON.stringify({
            id: 'chatcmpl-e2e-malformed',
            object: 'chat.completion.chunk',
            choices: [{
              index: 0,
              delta: {
                tool_calls: [{
                  index: 0,
                  id: 'call-malformed',
                  type: 'function',
                  function: { name: 'bash', arguments: '{not-json' },
                }],
              },
              finish_reason: 'tool_calls',
            }],
          })}\n\n`,
        )
      } else if (body?.user === 'guardrail-e2e-oversize-stream') {
        res.write(
          `data: ${JSON.stringify({
            id: 'chatcmpl-e2e-oversize',
            object: 'chat.completion.chunk',
            choices: [{ index: 0, delta: { content: 'x'.repeat(4 * 1024 * 1024) } }],
          })}\n\n`,
        )
      } else {
        res.write(
          `data: ${JSON.stringify({
            id: 'chatcmpl-e2e-stream',
            object: 'chat.completion.chunk',
            choices: [{ index: 0, delta: { content: 'pong' } }],
          })}\n\n`,
        )
      }
      res.end('data: [DONE]\n\n')
      return
    }

    if (body?.user === 'guardrail-e2e-tool-call') {
      sendJson(res, 200, {
        id: 'chatcmpl-e2e-tool',
        object: 'chat.completion',
        created: 1_741_510_000,
        model: body?.model ?? 'gpt-4o-mini',
        choices: [{
          index: 0,
          message: {
            role: 'assistant',
            content: null,
            tool_calls: [{
              id: 'call-e2e',
              type: 'function',
              function: {
                name: 'bash',
                arguments: '{"command":"rm -rf /tmp/oceans-response-e2e"}',
              },
            }],
          },
          finish_reason: 'tool_calls',
        }],
      })
      return
    }

    sendJson(res, 200, {
      id: 'chatcmpl-e2e-1',
      object: 'chat.completion',
      created: 1_741_510_000,
      model: body?.model ?? 'gpt-4o-mini',
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
        prompt_tokens: 80_000,
        completion_tokens: 40_000,
        total_tokens: 120_000,
      },
    })
    return
  }

  sendJson(res, 404, {
    error: {
      type: 'not_found',
      message: `Unhandled ${req.method} ${url.pathname}`,
    },
  })
})

server.listen(port, '127.0.0.1', () => {
  console.log(`Mock OpenAI-compatible upstream listening on http://127.0.0.1:${port}`)
})
