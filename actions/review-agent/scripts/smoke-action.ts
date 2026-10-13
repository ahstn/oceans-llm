import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdtempSync, writeFileSync } from 'node:fs'
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import type { AddressInfo } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

// Runs the source action in dry-run mode against a real pull request, with a
// deterministic mock Oceans API standing in for the control plane and model.
const repository = required('GITHUB_REPOSITORY')
const prNumber = required('PR_NUMBER')
const githubToken = required('GH_TOKEN')
const reviewWorkspace = required('REVIEW_WORKSPACE')

function required(name: string): string {
  const value = process.env[name]
  if (!value) throw new Error(`${name} is required`)
  return value
}

async function readJson(request: IncomingMessage): Promise<any> {
  let body = ''
  for await (const chunk of request) body += chunk
  return body ? JSON.parse(body) : {}
}

function send(response: ServerResponse, status: number, data: unknown) {
  response.writeHead(status, { 'content-type': 'application/json' })
  response.end(JSON.stringify({ data }))
}

function streamCompletion(response: ServerResponse, messages: { role: string }[]) {
  const done = messages.some((message) => message.role === 'tool')
  const review = {
    summary: 'Deterministic action smoke; no model assessment.',
    findings: [],
    degraded_features: [],
  }
  const delta = done
    ? { content: 'Review complete.' }
    : {
        tool_calls: [
          {
            index: 0,
            id: 'smoke-review',
            type: 'function',
            function: { name: 'submit_review', arguments: JSON.stringify(review) },
          },
        ],
      }
  response.writeHead(200, { 'content-type': 'text/event-stream' })
  for (const choice of [
    { index: 0, delta, finish_reason: null },
    { index: 0, delta: {}, finish_reason: done ? 'stop' : 'tool_calls' },
  ]) {
    const chunk = {
      id: 'smoke',
      object: 'chat.completion.chunk',
      created: 0,
      model: 'smoke-model',
      choices: [choice],
    }
    response.write(`data: ${JSON.stringify(chunk)}\n\n`)
  }
  response.end('data: [DONE]\n\n')
}

const server = createServer(async (request, response) => {
  const route = `${request.method} ${new URL(request.url ?? '/', 'http://127.0.0.1').pathname}`
  console.log(`mock oceans: ${route}`)
  try {
    const body = await readJson(request)
    switch (route) {
      case 'POST /v1/chat/completions':
        return streamCompletion(response, body.messages)
      case 'POST /api/v1/review-agent/action/config/resolve':
        return send(response, 200, {
          repository: body.repository,
          pull_request_id: 'smoke-pr',
          effective_config: {
            model_id: 'smoke-model',
            model_execution_mode: 'oceans',
            model_context_window_tokens: 32768,
            model_max_output_tokens: 1024,
            inline_review_enabled: true,
            pr_summary_enabled: true,
            diagrams_enabled: false,
            linked_issue_detection_enabled: false,
            linked_issue_assessment_enabled: false,
            max_inline_comments: 20,
            request_changes_on_high_severity: false,
          },
          overrides_applied: {},
          overrides_rejected: {},
          reporting: {},
        })
      case 'POST /api/v1/review-agent/action/runs':
        return send(response, 200, { run: { id: 'smoke-run', status: 'in_progress' } })
      case 'POST /api/v1/review-agent/action/runs/smoke-run/complete':
      case 'POST /api/v1/review-agent/action/runs/smoke-run/fail':
        return send(response, 200, { run: { id: 'smoke-run', status: 'succeeded' } })
      default:
        return send(response, 404, { message: `Unexpected mock Oceans route: ${route}` })
    }
  } catch (error) {
    send(response, 500, { message: error instanceof Error ? error.message : String(error) })
  }
})

const prResponse = await fetch(`https://api.github.com/repos/${repository}/pulls/${prNumber}`, {
  headers: { authorization: `Bearer ${githubToken}`, accept: 'application/vnd.github+json' },
})
if (!prResponse.ok) throw new Error(`Could not load PR #${prNumber}: HTTP ${prResponse.status}`)
const pr = await prResponse.json()
if (pr.head.repo?.full_name !== pr.base.repo.full_name) {
  throw new Error('Review-agent smoke only supports same-repository pull requests.')
}
const eventPath = join(mkdtempSync(join(tmpdir(), 'review-action-smoke-')), 'event.json')
writeFileSync(eventPath, JSON.stringify({ repository: pr.base.repo, pull_request: pr }))

server.listen(0, '127.0.0.1')
await once(server, 'listening')
const { port } = server.address() as AddressInfo

const action = spawn(process.execPath, ['--import', 'tsx', 'src/main.ts'], {
  stdio: 'inherit',
  env: {
    ...process.env,
    GITHUB_EVENT_NAME: 'pull_request',
    GITHUB_EVENT_PATH: eventPath,
    GITHUB_WORKSPACE: reviewWorkspace,
    'INPUT_OCEANS-URL': `http://127.0.0.1:${port}`,
    'INPUT_OCEANS-API-KEY': 'smoke-oceans-api-key',
    'INPUT_DRY-RUN': 'true',
    'INPUT_TIMEOUT-MINUTES': '2',
    'INPUT_GITHUB-TOKEN': githubToken,
  },
})
const [code] = await once(action, 'exit')
server.close()
process.exitCode = code ?? 1
