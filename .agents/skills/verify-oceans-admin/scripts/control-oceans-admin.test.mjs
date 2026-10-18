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

// Starts a process with two loopback listeners (gateway and UI ports) from `cwd`.
async function startListener(cwd) {
  const child = spawn(
    process.execPath,
    [
      '-e',
      `const net = require('node:net');
       const ports = [];
       for (let i = 0; i < 2; i++) {
         const server = net.createServer().listen(0, '127.0.0.1', () => {
           ports.push(server.address().port);
           if (ports.length === 2) console.log(ports.join(' '));
         });
       }`,
    ],
    { cwd, stdio: ['ignore', 'pipe', 'inherit'] },
  )
  const [gatewayPort, uiPort] = await new Promise((resolve, reject) => {
    child.once('error', reject)
    child.stdout.once('data', (chunk) => resolve(String(chunk).trim().split(' ')))
  })
  return { child, gatewayPort, uiPort }
}

async function recordRun(stateBase, runId, listener) {
  const runDir = path.join(stateBase, runId)
  await fs.mkdir(runDir, { recursive: true })
  await fs.writeFile(path.join(runDir, 'gateway-port'), `${listener.gatewayPort}\n`)
  await fs.writeFile(path.join(runDir, 'ui-port'), `${listener.uiPort}\n`)
  await fs.writeFile(path.join(runDir, 'gateway.pid'), `${listener.child.pid}\n`)
  await fs.writeFile(path.join(runDir, 'ui.pid'), `${listener.child.pid}\n`)
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

async function waitForExit(child) {
  if (child.exitCode !== null || child.signalCode !== null) return
  await new Promise((resolve) => child.once('exit', resolve))
}

test('doctor and cleanup leave a listener from another checkout alone', { skip: !hasTools }, async (t) => {
  const stateBase = await fs.mkdtemp(path.join(os.tmpdir(), 'oceans-verify-state-'))
  const otherCheckout = await fs.mkdtemp(path.join(os.tmpdir(), 'oceans-other-checkout-'))
  spawnSync('git', ['init', '--quiet', otherCheckout])
  const foreign = await startListener(otherCheckout)
  t.after(async () => {
    foreign.child.kill('SIGKILL')
    await fs.rm(stateBase, { recursive: true, force: true })
    await fs.rm(otherCheckout, { recursive: true, force: true })
  })
  await recordRun(stateBase, 'foreign-run', foreign)

  const doctor = runControl(stateBase, 'foreign-run', 'doctor')
  assert.notEqual(doctor.status, 0)
  assert.match(doctor.stderr, /does not belong to/)

  const cleanup = runControl(stateBase, 'foreign-run', 'cleanup')
  assert.equal(cleanup.status, 0, cleanup.stderr)
  assert.match(cleanup.stderr, /Not stopping PID \d+ from .*oceans-other-checkout-/)
  await new Promise((resolve) => setTimeout(resolve, 200))
  assert.ok(isAlive(foreign.child.pid), 'cleanup must not signal another checkout')
})

test('cleanup still stops a listener started from this checkout', { skip: !hasTools }, async (t) => {
  const stateBase = await fs.mkdtemp(path.join(os.tmpdir(), 'oceans-verify-state-'))
  const owned = await startListener(repoRoot)
  t.after(async () => {
    owned.child.kill('SIGKILL')
    await fs.rm(stateBase, { recursive: true, force: true })
  })
  await recordRun(stateBase, 'owned-run', owned)

  const cleanup = runControl(stateBase, 'owned-run', 'cleanup')
  assert.equal(cleanup.status, 0, cleanup.stderr)
  await waitForExit(owned.child)
  assert.equal(owned.child.signalCode, 'SIGTERM')
})
