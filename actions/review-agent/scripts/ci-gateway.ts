import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import {
  closeSync,
  existsSync,
  mkdirSync,
  openSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { join } from 'node:path'
import { setTimeout } from 'node:timers/promises'
import * as core from '@actions/core'

// A real, disposable Oceans control plane for the repository's live review job.
// Only evidence.json is safe to upload. State, configuration, and DB contain credentials.
const directory = join(required('RUNNER_TEMP'), 'oceans-review-gateway')
const url = 'http://127.0.0.1:38490'
const statePath = join(directory, 'state.json')
let cookie = ''

interface State {
  pid: number
  cookie: string
  repositoryId: string
}

interface ReviewRun {
  id: string
  status: string
  head_sha: string
  github_run_id: string
  model_key: string
  model_execution_mode: string
  managed_comment_id: string | null
  managed_comment_status: string | null
  inline_comments_created: number | null
  files_changed: number | null
  error_summary: string | null
}

interface RequestLog {
  provider_key: string
  model_key: string
  status_code: number | null
  total_tokens: number | null
  error_code: string | null
}

function required(name: string): string {
  const value = process.env[name]
  if (!value) throw new Error(`${name} is required`)
  return value
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const response = await fetch(`${url}${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...(cookie ? { cookie } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(30_000),
  })
  if (!response.ok) {
    const error = (await response.json()) as { error?: { message?: string } }
    throw new Error(
      `${method} ${path}: HTTP ${response.status}: ${error.error?.message ?? 'request failed'}`,
    )
  }
  const setCookie = response.headers.get('set-cookie')
  if (setCookie) cookie = setCookie.split(';')[0]
  return ((await response.json()) as { data: T }).data
}

async function waitForReady(deadline: number): Promise<void> {
  try {
    const response = await fetch(`${url}/readyz`, { signal: AbortSignal.timeout(1000) })
    if (response.ok) return
  } catch {
    // The listener is not ready during migration and bootstrap.
  }
  if (Date.now() >= deadline) throw new Error('Temporary Oceans gateway did not become ready')
  await setTimeout(500)
  return waitForReady(deadline)
}

async function start(): Promise<void> {
  if (existsSync(directory)) throw new Error('Temporary gateway directory already exists')
  mkdirSync(directory, { mode: 0o700 })
  const password = randomBytes(32).toString('hex')
  core.setSecret(password)
  const configPath = join(directory, 'gateway.json')
  writeFileSync(
    configPath,
    JSON.stringify({
      server: { bind: '127.0.0.1:38490', log_format: 'json' },
      database: { path: join(directory, 'gateway.db') },
      auth: {
        bootstrap_admin: {
          enabled: true,
          email: 'review-ci@local',
          password: `literal.${password}`,
          require_password_change: false,
        },
      },
      permissions: {
        platform_admins: { pages: ['review_agent'], actions: [], default_page: 'api_keys' },
      },
      providers: [
        {
          id: 'openrouter',
          type: 'openai_compat',
          base_url: 'https://openrouter.ai/api/v1',
          pricing_provider_id: 'openrouter',
          auth: { kind: 'bearer', token: 'env.OPENROUTER_API_KEY' },
        },
      ],
      models: [
        {
          id: 'openai/gpt-5.6-luna',
          routes: [{ provider: 'openrouter', upstream_model: 'openai/gpt-5.6-luna' }],
        },
      ],
    }),
    { mode: 0o600 },
  )
  const log = openSync(join(directory, 'gateway.log'), 'a', 0o600)
  const child = spawn(
    join(required('CARGO_TARGET_DIR'), 'debug/gateway'),
    ['--config', configPath, 'serve'],
    {
      detached: true,
      stdio: ['ignore', log, log],
      env: {
        PATH: process.env.PATH,
        HOME: process.env.HOME,
        OPENROUTER_API_KEY: required('OPENROUTER_API_KEY'),
        OCEANS_API_KEY_SECRET_ENCRYPTION_KEY: randomBytes(32).toString('base64'),
      },
    },
  )
  closeSync(log)
  if (!child.pid) throw new Error('Temporary gateway process did not start')
  child.unref()
  writeFileSync(join(directory, 'pid'), String(child.pid))
  await waitForReady(Date.now() + 60_000)
  await request('POST', '/api/v1/auth/login/password', { email: 'review-ci@local', password })
  core.setSecret(cookie)
  // Populate authoritative route limits before the action resolves its model.
  await request('POST', '/api/v1/admin/models/pricing-catalog/refresh', {})
  const team = await request<{ id: string }>('POST', '/api/v1/admin/identity/teams', {
    name: 'Review CI',
    admin_user_ids: [],
  })
  const account = await request<{ id: string }>('POST', '/api/v1/admin/identity/service-accounts', {
    team_id: team.id,
    name: 'Review CI',
  })
  await request('PUT', '/api/v1/admin/spend/budgets', {
    scope: { kind: 'service_account', service_account_id: account.id },
    cadence: 'daily',
    amount_usd: '2.0000',
    hard_limit: true,
    timezone: 'UTC',
  })
  const key = await request<{ raw_key: string }>('POST', '/api/v1/admin/api-keys', {
    name: 'Review CI',
    owner_kind: 'service_account',
    owner_service_account_id: account.id,
    model_grant_mode: 'explicit',
    model_keys: ['openai/gpt-5.6-luna'],
  })
  core.setSecret(key.raw_key)
  const [owner, name] = required('GITHUB_REPOSITORY').split('/')
  const repository = await request<{ repository: { id: string } }>(
    'POST',
    '/api/v1/admin/review-agent/repositories',
    {
      provider: 'github',
      owner,
      name,
      full_name: required('GITHUB_REPOSITORY'),
      service_account_id: account.id,
    },
  )
  writeFileSync(
    statePath,
    JSON.stringify({ pid: child.pid, cookie, repositoryId: repository.repository.id }),
    { mode: 0o600 },
  )
  core.setOutput('url', url)
  core.setOutput('api-key', key.raw_key)
  console.log('Temporary Oceans gateway and repository binding are ready.')
}

async function verify(): Promise<void> {
  const state = JSON.parse(readFileSync(statePath, 'utf8')) as State
  cookie = state.cookie
  const { items } = await request<{ items: ReviewRun[] }>(
    'GET',
    `/api/v1/admin/review-agent/repositories/${state.repositoryId}/runs`,
  )
  const run = items.find((item) => item.github_run_id === required('GITHUB_RUN_ID'))
  assert(run, 'The action must create an Oceans run')
  const logs = await request<{ items: RequestLog[] }>(
    'GET',
    '/api/v1/admin/observability/request-logs?page_size=100',
  )
  const successfulRequests = logs.items.filter(
    (item) =>
      item.provider_key === 'openrouter' &&
      item.model_key === 'openai/gpt-5.6-luna' &&
      item.status_code === 200,
  )
  const evidence = {
    run_id: run.id,
    status: run.status,
    head_sha: run.head_sha,
    github_run_id: run.github_run_id,
    model_key: run.model_key,
    model_execution_mode: run.model_execution_mode,
    managed_comment_id: run.managed_comment_id,
    managed_comment_status: run.managed_comment_status,
    inline_comments_created: run.inline_comments_created,
    files_changed: run.files_changed,
    successful_provider_requests: successfulRequests.length,
    provider_total_tokens: successfulRequests.reduce(
      (sum, item) => sum + (item.total_tokens ?? 0),
      0,
    ),
    provider_errors: logs.items
      .filter((item) => item.error_code)
      .map((item) => ({ status: item.status_code, code: item.error_code })),
  }
  writeFileSync(join(directory, 'evidence.json'), JSON.stringify(evidence, null, 2))
  console.log(JSON.stringify(evidence))
  assert.equal(run.status, 'succeeded')
  assert.equal(run.head_sha, required('REVIEW_HEAD_SHA'))
  assert.equal(run.model_key, 'openai/gpt-5.6-luna')
  assert.equal(run.model_execution_mode, 'oceans')
  assert.equal(run.managed_comment_status, 'succeeded')
  assert(run.managed_comment_id, 'A real GitHub summary comment must be published')
  assert(successfulRequests.length > 0, 'The gateway must record successful OpenRouter requests')
  assert(evidence.provider_total_tokens > 0, 'The provider must return non-zero token usage')
}

function stop(): void {
  const pidPath = join(directory, 'pid')
  if (existsSync(pidPath)) {
    try {
      process.kill(Number(readFileSync(pidPath, 'utf8')), 'SIGTERM')
    } catch {
      /* Already stopped. */
    }
  }
  // Keep only sanitized evidence for artifact upload; never upload gateway logs or the DB.
  for (const file of [
    'state.json',
    'gateway.json',
    'gateway.db',
    'gateway.db-wal',
    'gateway.db-shm',
    'gateway.log',
    'pid',
  ]) {
    rmSync(join(directory, file), { force: true })
  }
}

const command = process.argv[2]
if (command === 'start') await start()
else if (command === 'verify') await verify()
else if (command === 'stop') stop()
else throw new Error('Expected start, verify, or stop')
