import assert from 'node:assert/strict'
import { execFile, execFileSync } from 'node:child_process'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
import { actionRoot, invokePi } from '../src/pi'
import type { PiReviewRequest } from '../src/pi'

// A real Pi SDK session with all three packages, using a local deterministic
// OpenAI-compatible provider. No live provider credentials or GitHub writes.
const workspace = mkdtempSync(join(tmpdir(), 'review-sdk-smoke-'))
const git = (...args: string[]) =>
  execFileSync('git', args, { cwd: workspace, encoding: 'utf8' }).trim()
const requests: { tools?: { function: { name: string } }[]; messages: { content: unknown }[] }[] =
  []
let scenario: 'success' | 'invalid' | 'missing' | 'delegation' = 'success'
const lifecycle: string[] = []
const server = createServer(async (req, res) => {
  let body = ''
  for await (const chunk of req) body += chunk
  const input = JSON.parse(body)
  if (req.url?.startsWith('/api/v1/review-agent/action/')) {
    lifecycle.push(req.url)
    res.writeHead(200, { 'content-type': 'application/json' })
    const data = req.url.endsWith('/config/resolve')
      ? { effective_config: { model_id: 'smoke-model', model_execution_mode: 'oceans' } }
      : { run: { id: 'smoke-run' } }
    res.end(JSON.stringify({ data }))
    return
  }
  requests.push(input)
  res.writeHead(200, { 'content-type': 'text/event-stream' })
  const child = !input.tools?.some(
    (t: { function: { name: string } }) => t.function.name === 'submit_review',
  )
  const toolResponses = input.messages.filter((m: { role: string }) => m.role === 'tool').length
  const delegate = scenario === 'delegation' && !child && toolResponses === 0
  const toolDone =
    child || scenario === 'missing' || toolResponses >= (scenario === 'delegation' ? 2 : 1)
  const delta = toolDone
    ? { content: child ? 'CHILD_REVIEW_VERIFIED' : 'Review complete.' }
    : {
        tool_calls: [
          {
            index: 0,
            id: 'review-1',
            type: 'function',
            function: {
              name: delegate ? 'subagent' : 'submit_review',
              arguments: JSON.stringify(
                delegate
                  ? {
                      agent: 'reviewer',
                      task: 'Inspect example.ts and report its return value.',
                      async: false,
                      agentScope: 'user',
                      context: 'fresh',
                    }
                  : {
                      summary: 'Reviewed the changed return value.',
                      findings: [
                        {
                          path: 'example.ts',
                          line: scenario === 'invalid' ? 99 : 1,
                          severity: 'medium',
                          message: 'This returns the wrong value.',
                        },
                      ],
                      degraded_features: [],
                    },
              ),
            },
          },
        ],
      }
  res.write(
    `data: ${JSON.stringify({ id: 'smoke', object: 'chat.completion.chunk', created: 0, model: 'smoke-model', choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`,
  )
  res.write(
    `data: ${JSON.stringify({ id: 'smoke', object: 'chat.completion.chunk', created: 0, model: 'smoke-model', choices: [{ index: 0, delta: {}, finish_reason: toolDone ? 'stop' : 'tool_calls' }], usage: { prompt_tokens: 20, completion_tokens: 20, total_tokens: 40 } })}\n\n`,
  )
  res.end('data: [DONE]\n\n')
})
try {
  git('init', '-q')
  git('config', 'user.email', 'smoke@example.test')
  git('config', 'user.name', 'Smoke')
  writeFileSync(join(workspace, 'example.ts'), 'export const value = 1\n')
  git('add', '.')
  git('commit', '-qm', 'base')
  const base = git('rev-parse', 'HEAD')
  writeFileSync(join(workspace, 'example.ts'), 'export const value = 2\n')
  git('commit', '-qam', 'head')
  const head = git('rev-parse', 'HEAD')
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const address = server.address()
  assert(address && typeof address !== 'string')
  const request: PiReviewRequest = {
    workspace,
    oceansUrl: `http://127.0.0.1:${address.port}`,
    effectiveConfig: { model_id: 'smoke-model', model_execution_mode: 'oceans' },
    context: {
      repository: { provider: 'github', owner: 'test', name: 'repo', full_name: 'test/repo' },
      pullRequest: {
        pr_number: 1,
        base_sha: base,
        head_sha: head,
        is_draft: false,
        head_repository_full_name: 'test/repo',
        base_repository_full_name: 'test/repo',
      },
    },
  }
  const result = await invokePi(request, 'local-test-key', 2)
  assert.equal(result.findings[0]?.path, 'example.ts')
  const names = requests[0]?.tools?.map((t) => t.function.name) ?? []
  for (const name of ['submit_review', 'mcp', 'subagent', 'web_search'])
    assert(names.includes(name), `Missing tool ${name}: ${names.join(', ')}`)
  assert(!names.includes('bash') && !names.includes('write'))
  assert(JSON.stringify(requests[0]?.messages).includes('code-review'))
  scenario = 'invalid'
  await assert.rejects(invokePi(request, 'local-test-key', 2), /without calling submit_review/)
  scenario = 'missing'
  await assert.rejects(invokePi(request, 'local-test-key', 2), /without calling submit_review/)
  scenario = 'delegation'
  await invokePi(request, 'local-test-key', 2)
  assert(
    requests.some((r) => JSON.stringify(r.messages).includes('CHILD_REVIEW_VERIFIED')),
    'Foreground child result must reach the parent',
  )
  const childRequests = requests.filter(
    (r) => !r.tools?.some((t) => t.function.name === 'submit_review'),
  )
  assert(childRequests.length > 0)
  for (const childRequest of childRequests) {
    assert(
      childRequest.tools?.every((t) => ['read', 'grep', 'find', 'ls'].includes(t.function.name)),
    )
  }
  scenario = 'success'
  const eventPath = join(workspace, 'event.json')
  const summaryPath = join(workspace, 'summary.md')
  const repository = { id: 1, owner: { login: 'test' }, name: 'repo', full_name: 'test/repo' }
  writeFileSync(
    eventPath,
    JSON.stringify({
      repository,
      pull_request: {
        number: 1,
        draft: false,
        base: { sha: base, repo: repository },
        head: { sha: head, repo: repository },
      },
    }),
  )
  writeFileSync(summaryPath, '')
  await promisify(execFile)(
    process.execPath,
    ['--import', fileURLToPath(import.meta.resolve('tsx')), join(actionRoot, 'src/main.ts')],
    {
      cwd: actionRoot,
      env: {
        PATH: process.env.PATH,
        GITHUB_EVENT_NAME: 'pull_request',
        GITHUB_EVENT_PATH: eventPath,
        GITHUB_WORKSPACE: workspace,
        GITHUB_REPOSITORY: 'test/repo',
        GITHUB_STEP_SUMMARY: summaryPath,
        'INPUT_OCEANS-URL': request.oceansUrl,
        'INPUT_OCEANS-API-KEY': 'local-test-key',
        'INPUT_GITHUB-TOKEN': 'mock-token',
        'INPUT_DRY-RUN': 'true',
      },
      timeout: 120_000,
    },
  )
  assert.deepEqual(lifecycle, [
    '/api/v1/review-agent/action/config/resolve',
    '/api/v1/review-agent/action/runs',
    '/api/v1/review-agent/action/runs/smoke-run/complete',
  ])
  console.log(
    'Pi SDK smoke passed: installed extensions, provider round trip, validated review result.',
  )
} finally {
  server.closeAllConnections()
  server.close()
  rmSync(workspace, { recursive: true, force: true })
}
