import assert from 'node:assert/strict'
import { execFile, execFileSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
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
const requests: {
  max_tokens?: number
  max_completion_tokens?: number
  tools?: { function: { name: string } }[]
  messages: { content: unknown }[]
}[] = []
let scenario:
  | 'success'
  | 'invalid'
  | 'missing'
  | 'delegation'
  | 'provider-error'
  | 'retry'
  | 'boundary'
  | 'cancel' = 'success'
const lifecycle: string[] = []
const completions: { status?: string }[] = []
const isLinux = process.platform === 'linux'
const limits = { model_context_window_tokens: 32768, model_max_output_tokens: 1024 }
const runSdk = (request: PiReviewRequest) =>
  invokePi(request, 'local-test-key', 2, { sandbox: isLinux })
const outside = mkdtempSync(join(tmpdir(), 'review-outside-'))
const outsideSecret = join(outside, 'secret')
const forbiddenPaths = [
  '/proc/self/environ',
  '/proc/1/environ',
  outsideSecret,
  join(workspace, 'escape'),
]
let providerStarted: (() => void) | undefined
function nextCompletion(input: {
  tools?: { function: { name: string } }[]
  messages: { role: string }[]
}) {
  const child = !input.tools?.some(
    (t: { function: { name: string } }) => t.function.name === 'submit_review',
  )
  const toolResponses = input.messages.filter((m: { role: string }) => m.role === 'tool').length
  const delegate = scenario === 'delegation' && !child && toolResponses === 0
  const boundaryPath =
    scenario === 'boundary'
      ? forbiddenPaths[toolResponses]
      : scenario === 'delegation' && child && toolResponses === 0
        ? '/proc/self/environ'
        : undefined
  const toolDone =
    !boundaryPath &&
    (child ||
      scenario === 'missing' ||
      toolResponses >=
        (scenario === 'delegation' || scenario === 'retry'
          ? 2
          : scenario === 'boundary'
            ? forbiddenPaths.length + 1
            : 1))
  const delta = toolDone
    ? { content: child ? 'CHILD_REVIEW_VERIFIED' : 'Review complete.' }
    : {
        tool_calls: [
          {
            index: 0,
            id: 'review-1',
            type: 'function',
            function: {
              name: boundaryPath ? 'read' : delegate ? 'subagent' : 'submit_review',
              arguments: JSON.stringify(
                boundaryPath
                  ? { path: boundaryPath }
                  : delegate
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
                            line:
                              scenario === 'invalid' ||
                              (scenario === 'retry' && toolResponses === 0)
                                ? 99
                                : 1,
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
  return { delta, toolDone }
}

const server = createServer(async (req, res) => {
  let body = ''
  for await (const chunk of req) body += chunk
  const input = JSON.parse(body)
  if (req.headers.authorization !== 'Bearer local-test-key') {
    res.writeHead(401, { 'content-type': 'application/json' })
    res.end(JSON.stringify({ error: { message: 'SDK did not resolve the gateway credential' } }))
    return
  }
  if (scenario === 'provider-error') {
    res.writeHead(400, { 'content-type': 'application/json' })
    res.end(
      JSON.stringify({
        error: { message: 'Synthetic provider rejection', type: 'invalid_request_error' },
      }),
    )
    return
  }
  if (req.url?.startsWith('/api/v1/review-agent/action/')) {
    lifecycle.push(req.url)
    if (req.url.endsWith('/complete')) completions.push(input)
    res.writeHead(200, { 'content-type': 'application/json' })
    const data = req.url.endsWith('/config/resolve')
      ? {
          effective_config: {
            model_id: 'smoke-model',
            model_execution_mode: 'oceans',
            ...limits,
            linked_issue_detection_enabled: true,
          },
        }
      : { run: { id: 'smoke-run' } }
    res.end(JSON.stringify({ data }))
    return
  }
  requests.push(input)
  if (scenario === 'cancel') {
    providerStarted?.()
    return
  }
  res.writeHead(200, { 'content-type': 'text/event-stream' })
  const { delta, toolDone } = nextCompletion(input)
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
  // These files must stay review data, never runtime configuration.
  writeFileSync(join(workspace, 'tsconfig.json'), 'not valid JSON')
  mkdirSync(join(workspace, '.pi/extensions'), { recursive: true })
  writeFileSync(
    join(workspace, '.pi/extensions/poison.ts'),
    'throw new Error("PR_EXTENSION_EXECUTED")',
  )
  writeFileSync(outsideSecret, 'HOST_SECRET_MUST_NOT_REACH_MODEL')
  symlinkSync(outsideSecret, join(workspace, 'escape'))
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const address = server.address()
  assert(address && typeof address !== 'string')
  const request: PiReviewRequest = {
    workspace,
    oceansUrl: `http://127.0.0.1:${address.port}`,
    effectiveConfig: {
      model_id: 'smoke-model',
      model_execution_mode: 'oceans',
      ...limits,
      linked_issue_detection_enabled: true,
    },
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
  const result = await runSdk(request)
  assert.equal(result.findings[0]?.path, 'example.ts')
  assert.equal(result.metrics.files_changed, 1)
  assert.equal(result.metrics.linked_issue_status, 'degraded')
  assert(result.degradedFeatures.includes('linked_issue_detection'))
  assert.equal(requests[0]?.max_completion_tokens ?? requests[0]?.max_tokens, 1024)
  scenario = 'retry'
  assert.equal((await runSdk(request)).findings[0]?.line, 1)
  if (isLinux) {
    scenario = 'boundary'
    await runSdk(request)
    const visible = JSON.stringify(requests.map((request) => request.messages))
    assert(!visible.includes('HOST_SECRET_MUST_NOT_REACH_MODEL'))
    assert(!visible.includes('local-test-key'))
    assert(
      forbiddenPaths.every((path) => visible.includes(path)),
      'Every forbidden path must be probed',
    )
  }
  const names = requests[0]?.tools?.map((t) => t.function.name) ?? []
  for (const name of ['submit_review', 'mcp', 'subagent', 'web_search'])
    assert(names.includes(name), `Missing tool ${name}: ${names.join(', ')}`)
  assert(!names.includes('bash') && !names.includes('write'))
  assert(JSON.stringify(requests[0]?.messages).includes('code-review'))
  scenario = 'invalid'
  await assert.rejects(runSdk(request), /without calling submit_review/)
  scenario = 'missing'
  await assert.rejects(runSdk(request), /without calling submit_review/)
  scenario = 'provider-error'
  await assert.rejects(runSdk(request), /Synthetic provider rejection/)
  scenario = 'delegation'
  await runSdk(request)
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
  const launchLifecycle = () =>
    promisify(execFile)(
      process.execPath,
      ['--import', fileURLToPath(import.meta.resolve('tsx')), join(actionRoot, 'src/main.ts')],
      {
        cwd: actionRoot,
        env: {
          PATH: process.env.PATH,
          GITHUB_EVENT_NAME: 'pull_request_target',
          GITHUB_EVENT_PATH: eventPath,
          GITHUB_WORKSPACE: actionRoot,
          OCEANS_REVIEW_WORKSPACE: workspace,
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
  if (isLinux) {
    await launchLifecycle()
    assert.deepEqual(lifecycle, [
      '/api/v1/review-agent/action/config/resolve',
      '/api/v1/review-agent/action/runs',
      '/api/v1/review-agent/action/runs/smoke-run/complete',
    ])
    scenario = 'cancel'
    let timer: ReturnType<typeof setTimeout> | undefined
    const started = new Promise<void>((resolve, reject) => {
      timer = setTimeout(() => reject(new Error('Cancellation fixture did not start')), 30_000)
      providerStarted = resolve
    })
    const execution = launchLifecycle()
    const rejected = assert.rejects(execution)
    try {
      await started
      execution.child.kill('SIGTERM')
      await rejected
      assert.equal(completions.at(-1)?.status, 'cancelled')
    } finally {
      clearTimeout(timer)
      execution.child.kill('SIGKILL')
    }
  }
  if (!isLinux)
    console.log(
      'Linux sandbox and action lifecycle checks run in CI; local SDK uses only synthetic credentials.',
    )
  console.log(
    'Pi SDK smoke passed: installed extensions, provider round trip, validated review result.',
  )
} finally {
  server.closeAllConnections()
  server.close()
  rmSync(workspace, { recursive: true, force: true })
  rmSync(outside, { recursive: true, force: true })
}
