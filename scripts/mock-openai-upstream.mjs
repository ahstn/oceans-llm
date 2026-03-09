import { createServer } from 'node:http'

const port = Number(process.env.E2E_UPSTREAM_PORT ?? '18081')
const requests = []

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

  if (req.method === 'POST' && url.pathname === '/v1/chat/completions') {
    const body = await readJson(req)
    requests.push({
      method: req.method,
      path: url.pathname,
      headers: collectHeaders(req.headers),
      body,
    })

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
        prompt_tokens: 8,
        completion_tokens: 4,
        total_tokens: 12,
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
