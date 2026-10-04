import assert from 'node:assert/strict'
import { execFile, spawn } from 'node:child_process'
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'

const execFileAsync = promisify(execFile)
const script = fileURLToPath(new URL('./start-e2e-stack.sh', import.meta.url))

async function fixture(t, stackStatus, awsStatus) {
  const directory = await mkdtemp(path.join(tmpdir(), 'oceans-e2e-cleanup-test-'))
  t.after(() => rm(directory, { recursive: true, force: true }))
  const bin = path.join(directory, 'bin')
  await mkdir(bin)
  for (const [name, body] of Object.entries({
    mise: 'case "$*" in *"bun run build"*) exit 0;; *) exec sleep 30;; esac',
    curl: 'exit 0',
    aws: 'printf "%s\\n" "$*" >"$CLEANUP_TEST_DIRECTORY/aws-arguments"; echo "fixture object cleanup started" >&2; sleep 0.1; exit "$CLEANUP_TEST_AWS_STATUS"',
    gateway: stackStatus === 'signal' ? 'exec sleep 30' : `exit ${stackStatus}`,
  })) {
    await writeFile(path.join(bin, name), `#!/usr/bin/env bash\n${body}\n`, { mode: 0o700 })
  }
  return {
    directory,
    env: {
      ...process.env,
      PATH: `${bin}${path.delimiter}${process.env.PATH}`,
      TMPDIR: directory,
      MISE_BIN: path.join(bin, 'mise'),
      CARGO_TARGET_DIR: path.join(directory, 'target'),
      E2E_GATEWAY_BIN: path.join(bin, 'gateway'),
      E2E_SKILLS_ENABLED: 'true',
      E2E_PERMISSION_SCENARIO: 'default',
      CLEANUP_TEST_DIRECTORY: directory,
      CLEANUP_TEST_AWS_STATUS: String(awsStatus),
      OCEANS_SKILLS_S3_ENDPOINT: 'http://127.0.0.1:19999',
      OCEANS_SKILLS_S3_BUCKET: 'cleanup-test',
      OCEANS_SKILLS_S3_REGION: 'us-east-1',
      OCEANS_SKILLS_S3_ACCESS_KEY_ID: 'fixture-access',
      OCEANS_SKILLS_S3_SECRET_ACCESS_KEY: 'fixture-secret',
    },
  }
}

async function runStack(env) {
  try {
    const result = await execFileAsync('bash', [script], { env, timeout: 10_000 })
    return { ...result, status: 0 }
  } catch (error) {
    assert.equal(error.killed, false, 'fixture must exit before its timeout')
    assert.equal(typeof error.code, 'number')
    return { ...error, status: error.code }
  }
}

async function retainedState(directory, expected) {
  const entries = (await readdir(directory)).filter((entry) => entry.startsWith('oceans-e2e.'))
  assert.equal(entries.length, expected ? 1 : 0)
  const command = await readFile(path.join(directory, 'aws-arguments'), 'utf8')
  assert.match(command, /s3 rm s3:\/\/cleanup-test\/skills-e2e\/oceans-e2e\.[^/]+\/ --recursive/)
  if (expected) {
    const runtime = path.join(directory, entries[0])
    const config = await readFile(path.join(runtime, 'gateway.e2e.yaml'), 'utf8')
    assert.ok(config.includes(`prefix: "skills-e2e/${entries[0]}/"`))
    const retry = await readFile(path.join(runtime, 'skills-cleanup.txt'), 'utf8')
    assert.ok(retry.includes('endpoint=http://127.0.0.1:19999'))
    assert.ok(retry.includes('bucket=cleanup-test'))
    assert.ok(retry.includes(`prefix=skills-e2e/${entries[0]}/`))
    assert.ok(!retry.includes('fixture-secret'))
    return runtime
  }
}

for (const [stackStatus, awsStatus, expectedStatus, preserved] of [
  [0, 0, 0, false],
  [23, 0, 23, false],
  [0, 7, 1, true],
  [23, 7, 23, true],
]) {
  test(`stack status ${stackStatus}, AWS status ${awsStatus}: exit ${expectedStatus}, retain=${preserved}`, async (t) => {
    const { directory, env } = await fixture(t, stackStatus, awsStatus)
    const result = await runStack(env)
    assert.equal(result.status, expectedStatus, result.stderr)
    const runtime = await retainedState(directory, preserved)
    if (preserved) assert.ok(result.stderr.includes(runtime))
  })
}

test('repeated SIGTERM retains its status and retry state when object cleanup fails', async (t) => {
  const { directory, env } = await fixture(t, 'signal', 7)
  const child = spawn('bash', [script], { env, stdio: ['ignore', 'pipe', 'pipe'] })
  const status = await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      child.kill('SIGKILL')
      reject(new Error('signal fixture exceeded its timeout'))
    }, 10_000)
    let output = ''
    child.stdout.on('data', (chunk) => {
      output += chunk
      if (output.includes('E2E stack ready')) child.kill('SIGTERM')
    })
    child.stderr.on('data', (chunk) => {
      if (String(chunk).includes('fixture object cleanup started')) child.kill('SIGTERM')
    })
    child.on('error', (error) => {
      clearTimeout(timeout)
      reject(error)
    })
    child.on('exit', (code, signal) => {
      clearTimeout(timeout)
      signal ? reject(new Error(`unexpected signal: ${signal}`)) : resolve(code)
    })
  })
  assert.equal(status, 143)
  await retainedState(directory, true)
})
