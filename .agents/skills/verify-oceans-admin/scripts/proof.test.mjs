import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import { parseCheck, renderReport } from './proof-cli.mjs'
import { assessProof, buildEnvelope, sourceFingerprint, verdictFor } from './proof.mjs'

const cli = path.join(path.dirname(fileURLToPath(import.meta.url)), 'proof-cli.mjs')

test('a required failure beats a required block, and optional checks never decide', () => {
  assert.equal(verdictFor([{ id: 'a', status: 'pass' }]), 'pass')
  assert.equal(verdictFor([{ id: 'a', status: 'pass' }, { id: 'b', status: 'blocked' }]), 'blocked')
  assert.equal(verdictFor([{ id: 'a', status: 'blocked' }, { id: 'b', status: 'fail' }]), 'fail')
  assert.equal(verdictFor([{ id: 'a', status: 'pass' }, { id: 'b', status: 'fail', required: false }]), 'pass')
  assert.equal(verdictFor([]), 'fail')
  assert.equal(verdictFor([{ id: 'a', status: 'pass', required: false }]), 'fail')
})

test('check specs parse id, status, and an optional note', () => {
  assert.deepEqual(parseCheck('kpis-match-api=pass:4 of 4', true), {
    id: 'kpis-match-api',
    status: 'pass',
    required: true,
    note: '4 of 4',
  })
  assert.throws(() => parseCheck('kpis=passed', true), /id=pass\|fail\|blocked/)
})

test('envelopes reject unknown features and carry provenance', () => {
  assert.throws(() => buildEnvelope({ featureIds: ['nope'], checks: [] }), /Unknown feature IDs: nope/)
  const envelope = buildEnvelope({ featureIds: ['models'], checks: [{ id: 'a', status: 'pass' }] })
  assert.equal(envelope.schemaVersion, 1)
  assert.match(envelope.gitSha, /^[0-9a-f]{40}$/)
  assert.match(envelope.sourceHash, /^[0-9a-f]{64}$/)
  assert.ok(envelope.sourceFiles > 0)
  assert.equal(envelope.verdict, 'pass')
})

test('fingerprints are stable per feature and differ between features', () => {
  assert.equal(sourceFingerprint(['models']).sourceHash, sourceFingerprint(['models']).sourceHash)
  assert.notEqual(sourceFingerprint(['models']).sourceHash, sourceFingerprint(['mcp']).sourceHash)
})

test('assessment rejects legacy, failing, forged, foreign, and stale proofs', () => {
  const good = buildEnvelope({ featureIds: ['models'], checks: [{ id: 'a', status: 'pass' }] })
  good.runId = 'run-a'
  assert.deepEqual(assessProof(good, { requireCurrent: true, runId: 'run-a' }), [])
  assert.match(assessProof({ feature: 'models' })[0], /schemaVersion missing/)
  const failing = { ...good, verdict: 'fail', checks: [{ id: 'a', status: 'fail', note: 'count 3 != 4' }] }
  assert.match(assessProof(failing).join(), /verdict=fail \(a: count 3 != 4\)/)
  assert.match(assessProof({ ...good, verdict: 'pass', checks: [{ id: 'a', status: 'fail' }] }).join(), /does not match/)
  assert.match(assessProof(good, { runId: 'run-b' }).join(), /belongs to run run-a/)
  assert.match(assessProof({ ...good, sourceHash: '0'.repeat(64) }, { requireCurrent: true }).join(), /^stale/)
})

test('report lists planned features that have no passing proof', () => {
  const proof = buildEnvelope({ featureIds: ['models'], checks: [{ id: 'a', status: 'pass' }] })
  const blocked = buildEnvelope({
    featureIds: ['mcp'],
    checks: [{ id: 'candidate-exa', status: 'blocked', note: 'EXA key missing' }],
  })
  const markdown = renderReport({
    runId: 'run-a',
    proofs: [
      { file: 'models-proof.json', proof },
      { file: 'mcp-proof.json', proof: blocked },
    ],
    assessments: [[], ['verdict=blocked']],
    plan: { proofs: [{ id: 'models' }, { id: 'mcp' }, { id: 'usage-costs' }], unmapped: [], skipped: [] },
  })
  assert.match(markdown, /\| models \| ✅ pass \| 1\/1 \| models-proof.json \|/)
  assert.match(markdown, /\| mcp \| ⛔ blocked \| 0\/1; candidate-exa: blocked, EXA key missing \|/)
  assert.match(markdown, /\*\*Planned but not verified:\*\* mcp, usage-costs/)
})

test('record writes a manual proof that evidence accepts, and evidence fails on a missing feature', async () => {
  const evidenceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'oceans-proof-'))
  const env = { ...process.env, OCEANS_VERIFY_RUN_ID: 'proof-test', OCEANS_VERIFY_EVIDENCE_DIR: evidenceDir }
  const run = (...args) => spawnSync(process.execPath, [cli, ...args], { env, encoding: 'utf8' })
  try {
    assert.equal(run('record', 'usage-costs', '--check', 'kpis=pass').status, 2, 'artifacts are required')
    await fs.writeFile(path.join(evidenceDir, '01-usage.png'), '')
    const recorded = run('record', 'usage-costs', '--check', 'kpis-match-api=pass:4 of 4', '--artifact', '01-usage.png')
    assert.equal(recorded.status, 0, recorded.stderr)
    const saved = JSON.parse(await fs.readFile(path.join(evidenceDir, 'usage-costs-proof.json'), 'utf8'))
    assert.equal(saved.method, 'manual')
    assert.deepEqual(saved.artifacts, ['01-usage.png'])
    const current = run('evidence', 'usage-costs', '--require-current')
    assert.equal(current.status, 0, current.stdout)
    assert.match(current.stdout, /PASS usage-costs \(usage-costs-proof.json\)/)
    const missing = run('evidence', 'usage-costs', 'models')
    assert.equal(missing.status, 1)
    assert.match(missing.stdout, /FAIL models: no proof recorded/)
  } finally {
    await fs.rm(evidenceDir, { recursive: true, force: true })
  }
})
