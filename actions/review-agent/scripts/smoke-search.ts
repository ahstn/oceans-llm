import assert from 'node:assert/strict'
import { copyFileSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

// Exercise the installed package's routing with no real credentials or network.
const agentDir = mkdtempSync(join(tmpdir(), 'review-search-smoke-'))
for (const name of Object.keys(process.env)) delete process.env[name]
process.env.HOME = agentDir
process.env.PI_CODING_AGENT_DIR = agentDir
copyFileSync(
  new URL('../config/web-search.json', import.meta.url),
  join(agentDir, 'web-search.json'),
)

const requests: string[] = []
let exaStatus = 429
globalThis.fetch = (async (input, init) => {
  const parsed = new URL(input instanceof Request ? input.url : input.toString())
  const url = `${parsed.origin}${parsed.pathname}`
  requests.push(url)
  const headers = new Headers(init?.headers)
  assert(!headers.has('authorization') && !headers.has('x-api-key'))
  if (url === 'https://mcp.exa.ai/mcp') return new Response('Unavailable', { status: exaStatus })
  assert.equal(url, 'https://search.parallel.ai/mcp', 'Fallback must use anonymous Parallel MCP')
  return Response.json({
    jsonrpc: '2.0',
    id: 1,
    result: {
      structuredContent: {
        results: [
          {
            title: 'Search documentation',
            url: 'https://example.com/search',
            excerpts: ['Search evidence'],
          },
        ],
      },
    },
  })
}) as typeof fetch

try {
  // Resolve from the pinned package, not a second implementation of its routing rules.
  const modulePath = fileURLToPath(
    new URL('../node_modules/pi-web-access/gemini-search.ts', import.meta.url),
  )
  const { search, getConfiguredSearchRouting } = await import(modulePath)
  assert.equal(getConfiguredSearchRouting().useCurrentModel, true)
  const result = await search('Search documentation')
  assert.equal(result.provider, 'parallel-mcp')
  assert.equal(result.results[0].url, 'https://example.com/search')
  assert.deepEqual(requests, ['https://mcp.exa.ai/mcp', 'https://search.parallel.ai/mcp'])

  requests.length = 0
  exaStatus = 401
  await assert.rejects(search('Search documentation'), /401/)
  assert.deepEqual(
    requests,
    ['https://mcp.exa.ai/mcp'],
    'Authentication failures must remain visible',
  )
  console.log('Search routing smoke passed: anonymous quota fallback and visible auth failures.')
} finally {
  rmSync(agentDir, { recursive: true, force: true })
}
