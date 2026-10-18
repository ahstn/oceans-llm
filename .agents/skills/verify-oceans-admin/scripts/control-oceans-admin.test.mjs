import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const scripts = path.dirname(fileURLToPath(import.meta.url))
const control = path.join(scripts, 'control-oceans-admin')
const repoRoot = path.resolve(scripts, '../../../..')
const hasTools = ['lsof', 'git', 'bash'].every(
  (tool) => spawnSync('sh', ['-c', `command -v ${tool}`]).status === 0,
)

// Starts one loopback listener from `cwd` and resolves with its process and port.
async function startListener(cwd) {
  const child = spawn(
    process.execPath,
    [
      '-e',
      `const server = require('node:net').createServer();
       server.listen(0, '127.0.0.1', () => console.log(server.address().port));`,
    ],
    { cwd, stdio: ['ignore', 'pipe', 'inherit'] },
  )
  const port = await new Promise((resolve, reject) => {
    const onExit = (code, signal) =>
      reject(new Error(`listener exited before reporting its port (code ${code}, signal ${signal})`))
    child.once('error', reject)
    child.once('exit', onExit)
    child.stdout.once('data', (chunk) => {
      child.off('error', reject)
      child.off('exit', onExit)
      resolve(String(chunk).trim())
    })
  })
  return { child, port }
}

async function recordRun(stateBase, runId, gateway, ui) {
  const runDir = path.join(stateBase, runId)
  await fs.mkdir(runDir, { recursive: true })
  await fs.writeFile(path.join(runDir, 'gateway-port'), `${gateway.port}\n`)
  await fs.writeFile(path.join(runDir, 'ui-port'), `${ui.port}\n`)
  await fs.writeFile(path.join(runDir, 'gateway.pid'), `${gateway.child.pid}\n`)
  await fs.writeFile(path.join(runDir, 'ui.pid'), `${ui.child.pid}\n`)
}

function runControl(stateBase, runId, command) {
  return spawnSync('bash', [control, command], {
    env: { ...process.env, OCEANS_VERIFY_STATE_BASE: stateBase, OCEANS_VERIFY_RUN_ID: runId },
    encoding: 'utf8',
    // Trusting a foreign listener used to hang on its readiness probe; fail instead of waiting.
    timeout: 20_000,
  })
}

function isAlive(pid) {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

async function exitSignal(child, timeoutMs = 5_000) {
  if (child.exitCode !== null || child.signalCode !== null) return child.signalCode
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve('still running'), timeoutMs)
    child.once('exit', (_code, signal) => {
      clearTimeout(timer)
      resolve(signal)
    })
  })
}

async function tempGitCheckout(t) {
  const checkout = await fs.mkdtemp(path.join(os.tmpdir(), 'oceans-other-checkout-'))
  spawnSync('git', ['init', '--quiet', checkout])
  t.after(() => fs.rm(checkout, { recursive: true, force: true }))
  return checkout
}

async function tempStateBase(t) {
  const stateBase = await fs.mkdtemp(path.join(os.tmpdir(), 'oceans-verify-state-'))
  t.after(() => fs.rm(stateBase, { recursive: true, force: true }))
  return stateBase
}

async function listenerFor(t, cwd) {
  const listener = await startListener(cwd)
  t.after(() => listener.child.kill('SIGKILL'))
  return listener
}

test('doctor and cleanup leave listeners from another checkout alone', { skip: !hasTools }, async (t) => {
  const stateBase = await tempStateBase(t)
  const otherCheckout = await tempGitCheckout(t)
  const gateway = await listenerFor(t, otherCheckout)
  const ui = await listenerFor(t, otherCheckout)
  await recordRun(stateBase, 'foreign-run', gateway, ui)

  const doctor = runControl(stateBase, 'foreign-run', 'doctor')
  assert.notEqual(doctor.status, 0)
  assert.match(doctor.stderr, /does not belong to/)

  const cleanup = runControl(stateBase, 'foreign-run', 'cleanup')
  assert.equal(cleanup.status, 0, cleanup.stderr)
  assert.match(cleanup.stderr, /Not stopping PID \d+ from .*oceans-other-checkout-/)
  await new Promise((resolve) => setTimeout(resolve, 200))
  assert.ok(isAlive(gateway.child.pid), 'cleanup must not signal another checkout')
  assert.ok(isAlive(ui.child.pid), 'cleanup must not signal another checkout')
})

test('an inherited GIT_DIR cannot make a foreign listener look owned', { skip: !hasTools }, async (t) => {
  const stateBase = await tempStateBase(t)
  const otherCheckout = await tempGitCheckout(t)
  const gateway = await listenerFor(t, otherCheckout)
  const ui = await listenerFor(t, otherCheckout)
  await recordRun(stateBase, 'git-env-run', gateway, ui)

  const cleanup = spawnSync('bash', [control, 'cleanup'], {
    env: {
      ...process.env,
      OCEANS_VERIFY_STATE_BASE: stateBase,
      OCEANS_VERIFY_RUN_ID: 'git-env-run',
      GIT_DIR: path.join(repoRoot, '.git'),
      GIT_WORK_TREE: repoRoot,
    },
    encoding: 'utf8',
    timeout: 20_000,
  })
  assert.equal(cleanup.status, 0, cleanup.stderr)
  await new Promise((resolve) => setTimeout(resolve, 200))
  assert.ok(isAlive(gateway.child.pid), 'GIT_DIR must not redirect the ownership check')
})

test('cleanup stops the owned listener and spares the foreign one', { skip: !hasTools }, async (t) => {
  const stateBase = await tempStateBase(t)
  const otherCheckout = await tempGitCheckout(t)
  const gateway = await listenerFor(t, repoRoot)
  const ui = await listenerFor(t, otherCheckout)
  await recordRun(stateBase, 'mixed-run', gateway, ui)

  const cleanup = runControl(stateBase, 'mixed-run', 'cleanup')
  assert.equal(cleanup.status, 0, cleanup.stderr)
  assert.equal(await exitSignal(gateway.child), 'SIGTERM')
  assert.ok(isAlive(ui.child.pid), 'cleanup must not signal another checkout')
})

test('cleanup stops gateway and UI listeners started from this checkout', { skip: !hasTools }, async (t) => {
  const stateBase = await tempStateBase(t)
  const gateway = await listenerFor(t, repoRoot)
  const ui = await listenerFor(t, path.join(repoRoot, 'crates/admin-ui/web'))
  await recordRun(stateBase, 'owned-run', gateway, ui)

  const cleanup = runControl(stateBase, 'owned-run', 'cleanup')
  assert.equal(cleanup.status, 0, cleanup.stderr)
  assert.equal(await exitSignal(gateway.child), 'SIGTERM')
  assert.equal(await exitSignal(ui.child), 'SIGTERM')
})
