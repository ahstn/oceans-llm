import assert from 'node:assert/strict'
import test from 'node:test'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { assessResults, requiredChecks, regressionFingerprint } from './messages-regressions.mjs'
import { assessProof, buildEnvelope } from './proof.mjs'

const passed = requiredChecks.map(([, name]) => `test suite::${name} ... ok`).join('\n')

test('Messages gate requires every named regression and a successful Rust process', () => {
  assert.equal(assessResults(passed, 0).verdict, 'pass')
  assert.equal(assessResults(passed, 1).verdict, 'fail')
  assert.equal(assessResults(passed, null).verdict, 'fail')
})

test('empty filters, ignored tests and missing regressions cannot produce a passing gate', () => {
  assert.equal(assessResults('test result: ok. 0 passed', 0).verdict, 'fail')
  assert.equal(assessResults(passed.replace(' ... ok', ' ... ignored'), 0).verdict, 'fail')
  assert.equal(assessResults(passed.split('\n').slice(1).join('\n'), 0).verdict, 'fail')
})

test('offline fingerprints include test assertions and suite dependencies', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'messages-fingerprint-'))
  try {
    execFileSync('git', ['init', '-q', root])
    await fs.mkdir(path.join(root, 'crates/gateway-service'), { recursive: true })
    const file = path.join(root, 'crates/gateway-service/request_tests.rs')
    await fs.writeFile(file, 'assert_eq!(actual, expected);')
    const before = regressionFingerprint(root).sourceHash
    await fs.writeFile(file, '// assertion removed')
    assert.notEqual(regressionFingerprint(root).sourceHash, before)
  } finally { await fs.rm(root, { recursive: true, force: true }) }
})

test('manual Messages proof cannot hide a missing, failed, incomplete or stale gate', async () => {
  const evidenceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'messages-evidence-'))
  const file = path.join(evidenceDir, 'messages-regressions.json')
  try {
    const proof = buildEnvelope({ featureIds: ['anthropic-messages'],
      checks: [{ id: 'live-response', status: 'pass' }], artifacts: ['messages-regressions.json'] })
    proof.launchSourceHash = proof.sourceHash
    const assess = () => assessProof(proof, { evidenceDir, requireCurrent: true }).join('\n')
    assert.match(assess(), /missing or unreadable/)
    const good = { schemaVersion: 1, ...regressionFingerprint(), ...assessResults(passed, 0) }
    await fs.writeFile(file, JSON.stringify(good))
    assert.equal(assess(), '')
    for (const replacement of [
      null,
      { ...good, verdict: 'fail' },
      { ...good, checks: 'invalid' },
      { ...good, checks: [null] },
      { ...good, checks: good.checks.slice(1) },
      { ...good, sourceHash: 'stale' },
    ]) {
      await fs.writeFile(file, JSON.stringify(replacement))
      assert.match(assess(), /did not pass|stale: Messages/)
    }
  } finally { await fs.rm(evidenceDir, { recursive: true, force: true }) }
})
