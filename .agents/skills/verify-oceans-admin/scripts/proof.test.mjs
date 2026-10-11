import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import { parseCheck, renderReport, reportRows } from './proof-cli.mjs'
import { assessProof, buildEnvelope, createProofRecorder, currentSources, sourceFingerprint, verdictFor } from './proof.mjs'

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
  good.launchSourceHash = good.sourceHash
  assert.match(assessProof({ ...good, launchSourceHash: null }, { requireCurrent: true }).join(), /not tied to a launched stack/)
  assert.deepEqual(assessProof(good, { requireCurrent: true, runId: 'run-a' }), [])
  assert.match(assessProof({ feature: 'models' })[0], /schemaVersion missing/)
  const failing = { ...good, verdict: 'fail', checks: [{ id: 'a', status: 'fail', note: 'count 3 != 4' }] }
  assert.match(assessProof(failing).join(), /verdict=fail \(a: count 3 != 4\)/)
  assert.match(assessProof({ ...good, verdict: 'pass', checks: [{ id: 'a', status: 'fail' }] }).join(), /does not match/)
  assert.match(assessProof(good, { runId: 'run-b' }).join(), /belongs to run run-a/)
  assert.match(assessProof({ ...good, sourceHash: '0'.repeat(64) }, { requireCurrent: true }).join(), /^stale/)
})

test('fingerprints include each file mode, so a mode-only change goes stale', () => {
  const sources = currentSources()
  const [file, entry] = Object.entries(sources).find(([name]) => name.startsWith('crates/admin-ui/'))
  assert.match(entry, /^100(644|755) [0-9a-f]{40}$/)
  const flipped = { ...sources, [file]: entry.replace(/^100(644|755)/, (mode) => (mode === '100644' ? '100755' : '100644')) }
  assert.notEqual(sourceFingerprint(['models'], undefined, flipped).sourceHash, sourceFingerprint(['models'], undefined, sources).sourceHash)
})

test('assessment rejects missing artifacts and proofs whose stack ran older source', async () => {
  const evidenceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'oceans-proof-'))
  try {
    const proof = buildEnvelope({ featureIds: ['models'], checks: [{ id: 'a', status: 'pass' }], artifacts: ['01.png'] })
    assert.match(assessProof(proof, { evidenceDir }).join(), /missing artifacts: 01.png/)
    await fs.writeFile(path.join(evidenceDir, '01.png'), '')
    proof.launchSourceHash = proof.sourceHash
    assert.deepEqual(assessProof(proof, { evidenceDir, requireCurrent: true }), [])
    assert.match(assessProof({ ...proof, artifacts: ['../01.png'] }, { evidenceDir }).join(), /missing artifacts/)
    const relaunchNeeded = { ...proof, launchSourceHash: '0'.repeat(64) }
    assert.match(assessProof(relaunchNeeded, { evidenceDir, requireCurrent: true }).join(), /^stale: .*changed after launch/)
  } finally {
    await fs.rm(evidenceDir, { recursive: true, force: true })
  }
})

test('a recorder replaces an earlier passing proof before the driver can fail', async () => {
  const evidenceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'oceans-proof-'))
  try {
    const first = createProofRecorder({ evidenceDir, file: 'models-proof.json', featureIds: ['models'] })
    first.pass('a')
    await first.write()
    createProofRecorder({ evidenceDir, file: 'models-proof.json', featureIds: ['models'] })
    const saved = JSON.parse(await fs.readFile(path.join(evidenceDir, 'models-proof.json'), 'utf8'))
    assert.equal(saved.verdict, 'fail')
    assert.equal(saved.checks[0].note, 'driver started but did not finish')
  } finally {
    await fs.rm(evidenceDir, { recursive: true, force: true })
  }
})

test('report lists planned features that have no passing proof', () => {
  const proof = buildEnvelope({ featureIds: ['models'], checks: [{ id: 'a', status: 'pass' }] })
  const blocked = buildEnvelope({
    featureIds: ['mcp'],
    checks: [{ id: 'candidate-exa', status: 'blocked', note: 'EXA key missing' }],
  })
  const proofs = [
    { file: 'models-proof.json', proof },
    { file: 'mcp-proof.json', proof: blocked },
  ]
  const markdown = renderReport({
    runId: 'run-a',
    rows: reportRows(proofs, (entry) => (entry === blocked ? ['verdict=blocked'] : [])),
    plan: { proofs: [{ id: 'models' }, { id: 'mcp' }, { id: 'usage-costs' }], unmapped: [], skipped: [] },
  })
  assert.match(markdown, /\| models \| ✅ pass \| 1\/1 \| models-proof.json \|/)
  assert.match(markdown, /\| mcp \| ⛔ blocked \| 0\/1; candidate-exa: blocked, EXA key missing \|/)
  assert.match(markdown, /\*\*Planned but not verified:\*\* mcp, usage-costs/)
})

test('report uses the newest proof per feature and marks invalid proofs', () => {
  const older = { ...buildEnvelope({ featureIds: ['models'], checks: [{ id: 'a', status: 'pass' }] }), generatedAt: '2026-01-01T00:00:00Z' }
  const newer = { ...buildEnvelope({ featureIds: ['models'], checks: [{ id: 'a', status: 'fail' }] }), generatedAt: '2026-01-02T00:00:00Z' }
  const foreign = buildEnvelope({ featureIds: ['mcp'], checks: [{ id: 'a', status: 'pass' }] })
  const proofs = [
    { file: 'models-proof.json', proof: older },
    { file: 'models-rerun-proof.json', proof: newer },
    { file: 'mcp-proof.json', proof: foreign },
  ]
  const assess = (entry) => (entry === newer ? ['verdict=fail (a)'] : entry === foreign ? ['proof belongs to run other, not run-a'] : [])
  const markdown = renderReport({
    runId: 'run-a',
    rows: reportRows(proofs, assess),
    plan: { proofs: [{ id: 'models' }, { id: 'mcp' }], unmapped: [], skipped: [] },
  })
  assert.doesNotMatch(markdown, /models-proof.json/)
  assert.match(markdown, /\| models \| ❌ fail \| 0\/1; a: fail \| models-rerun-proof.json \|/)
  assert.match(markdown, /\| mcp \| ❌ invalid: proof belongs to run other, not run-a \|/)
  assert.match(markdown, /\*\*Planned but not verified:\*\* models, mcp/)
})

test('record writes a manual proof that evidence accepts, and evidence fails on a missing feature', async () => {
  const evidenceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'oceans-proof-'))
  const launchSources = path.join(evidenceDir, 'launch-sources.json')
  const env = { ...process.env, OCEANS_VERIFY_RUN_ID: 'proof-test', OCEANS_VERIFY_EVIDENCE_DIR: evidenceDir }
  const run = (...args) => spawnSync(process.execPath, [cli, ...args], { env, encoding: 'utf8' })
  try {
    assert.equal(run('snapshot-sources', launchSources).status, 0)
    env.OCEANS_VERIFY_LAUNCH_SOURCES = launchSources
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
    const misspelled = run('evidence', 'usage-costs', '--require-curent')
    assert.equal(misspelled.status, 2)
    assert.match(misspelled.stderr, /Unknown evidence option --require-curent/)
    const missing = run('evidence', 'usage-costs', 'models')
    assert.equal(missing.status, 1)
    assert.match(missing.stdout, /FAIL models: no proof recorded/)
    const escaped = run('record', 'usage-costs', '--check', 'a=pass', '--artifact', '01-usage.png', '--file', '../x-proof.json')
    assert.equal(escaped.status, 2)
    assert.match(escaped.stderr, /--file must be a file name/)
    assert.equal(run('add-check', 'usage-costs-proof.json', 'storage-cleanup=fail:prefix retained').status, 0)
    const cleaned = JSON.parse(await fs.readFile(path.join(evidenceDir, 'usage-costs-proof.json'), 'utf8'))
    assert.equal(cleaned.verdict, 'fail')
    assert.equal(run('evidence', 'usage-costs').status, 1)
    await fs.rm(path.join(evidenceDir, '01-usage.png'))
    assert.equal(run('add-check', 'usage-costs-proof.json', 'storage-cleanup=pass').status, 0)
    assert.match(run('evidence', 'usage-costs').stdout, /FAIL usage-costs .*missing artifacts: 01-usage.png/)
  } finally {
    await fs.rm(evidenceDir, { recursive: true, force: true })
  }
})
