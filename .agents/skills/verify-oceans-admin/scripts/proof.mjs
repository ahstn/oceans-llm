// Shared proof envelope. Drivers and `control-oceans-admin record` write it;
// `evidence` and `report` read it. See SKILL.md "Proofs" for the contract.
import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { existsSync, lstatSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { planFor } from './plan-verification.mjs'

export const SCHEMA_VERSION = 1
export const STATUSES = ['pass', 'fail', 'blocked']
export const skillDir = path.join(path.dirname(fileURLToPath(import.meta.url)), '..')
export const repoRoot = path.join(skillDir, '../../..')

export function loadRouting() {
  return JSON.parse(readFileSync(path.join(skillDir, 'features/routing.json'), 'utf8'))
}

// A required failure beats a required block; optional checks never decide the verdict.
export function verdictFor(checks) {
  const required = checks.filter((check) => check.required !== false)
  if (required.length === 0) return 'fail'
  if (required.some((check) => check.status === 'fail')) return 'fail'
  if (required.some((check) => check.status === 'blocked')) return 'blocked'
  return 'pass'
}

// Content hash of every file that routing.json sends to these features, so a
// proof survives commits and unrelated edits but goes stale when its code changes.
// Each entry is path, Git file mode, and blob, so a mode-only change also counts.
export function sourceFingerprint(featureIds, routing = loadRouting(), sources = currentSources(routing)) {
  const wanted = new Set(featureIds)
  const routed = new Set(
    planFor(Object.keys(sources), routing)
      .proofs.filter((proof) => wanted.has(proof.id))
      .flatMap((proof) => proof.because),
  )
  const present = [...routed].sort()
  const hash = createHash('sha256')
  present.forEach((file) => hash.update(`${file}\0${sources[file]}\n`))
  return { sourceHash: hash.digest('hex'), sourceFiles: present.length }
}

// `<mode> <blob>` for every present file routed to any feature. `launch` saves
// this snapshot so proofs can show which source the running stack was built from.
export function currentSources(routing = loadRouting()) {
  const files = git(['ls-files', '--cached', '--others', '--exclude-standard'])
    .split('\n')
    .filter(Boolean)
  const routed = new Set(planFor(files, routing).proofs.flatMap((proof) => proof.because))
  const present = [...routed].filter((file) => existsSync(path.join(repoRoot, file))).sort()
  const blobs = present.length
    ? git(['hash-object', '--stdin-paths'], present.join('\n')).split('\n')
    : []
  return Object.fromEntries(present.map((file, index) => [file, `${fileMode(file)} ${blobs[index]}`]))
}

function fileMode(file) {
  const stat = lstatSync(path.join(repoRoot, file))
  if (stat.isSymbolicLink()) return '120000'
  return stat.mode & 0o111 ? '100755' : '100644'
}

// The snapshot `launch` wrote for this run, or null when the run has none.
export function launchSources() {
  const file = process.env.OCEANS_VERIFY_LAUNCH_SOURCES
  if (!file || !existsSync(file)) return null
  return JSON.parse(readFileSync(file, 'utf8'))
}

export function buildEnvelope({ featureIds, checks, details = {}, artifacts = [], method = 'driver', routing = loadRouting() }) {
  const known = new Set(routing.features.map((feature) => feature.id))
  const unknown = featureIds.filter((id) => !known.has(id))
  if (featureIds.length === 0 || unknown.length > 0) {
    throw new Error(`Unknown feature IDs: ${unknown.join(', ') || '(none)'}; use ids from features/routing.json`)
  }
  for (const check of checks) {
    if (!check.id || !STATUSES.includes(check.status)) throw new Error(`Invalid check ${JSON.stringify(check)}`)
  }
  const launched = launchSources()
  return {
    schemaVersion: SCHEMA_VERSION,
    runId: process.env.OCEANS_VERIFY_RUN_ID ?? null,
    gitSha: git(['rev-parse', 'HEAD']),
    dirty: git(['status', '--porcelain']).length > 0,
    ...sourceFingerprint(featureIds, routing),
    launchSourceHash: launched ? sourceFingerprint(featureIds, routing, launched).sourceHash : null,
    harness: detectHarness(),
    gatewayVersion: process.env.OCEANS_VERIFY_GATEWAY_VERSION ?? null,
    featureIds,
    method,
    verdict: verdictFor(checks),
    checks,
    artifacts,
    generatedAt: new Date().toISOString(),
    details,
  }
}

export async function writeProof(evidenceDir, file, envelope) {
  if (!file.endsWith('-proof.json')) throw new Error(`Proof files must end in -proof.json: ${file}`)
  await fs.mkdir(evidenceDir, { recursive: true })
  await fs.writeFile(path.join(evidenceDir, file), `${JSON.stringify(envelope, null, 2)}\n`)
  return envelope
}

// Driver helper: record checks as assertions pass, then write once at the end,
// including when the driver throws, so a failed run leaves a failed proof.
// Creating the recorder replaces any earlier proof in this file with a failing
// placeholder, so a crash before write() (browser launch included) cannot leave
// an older passing attempt in place.
export function createProofRecorder({ evidenceDir, file, featureIds }) {
  writePlaceholder(evidenceDir, file, featureIds)
  const checks = []
  const artifacts = []
  return {
    checks,
    artifacts,
    pass(id, fields = {}) {
      checks.push({ id, status: 'pass', ...fields })
    },
    add(check) {
      checks.push(check)
    },
    artifact(name) {
      if (!artifacts.includes(name)) artifacts.push(name)
    },
    async write({ details, error } = {}) {
      if (error) checks.push({ id: 'driver-completed', status: 'fail', note: safeMessage(error) })
      const envelope = buildEnvelope({ featureIds, checks, details, artifacts })
      return writeProof(evidenceDir, file, envelope)
    },
  }
}

function writePlaceholder(evidenceDir, file, featureIds) {
  if (!file.endsWith('-proof.json')) throw new Error(`Proof files must end in -proof.json: ${file}`)
  const checks = [{ id: 'driver-completed', status: 'fail', note: 'driver started but did not finish' }]
  mkdirSync(evidenceDir, { recursive: true })
  writeFileSync(path.join(evidenceDir, file), `${JSON.stringify(buildEnvelope({ featureIds, checks }), null, 2)}\n`)
}

// Error text can echo upstream response bodies; keep the label, drop payloads.
export function safeMessage(error) {
  const message = String(error?.name === 'TimeoutError' ? 'Timed out waiting for the page' : error?.message ?? error)
  return message.split(/[{\n]/)[0].trim().slice(0, 200)
}

export async function readProofs(evidenceDir) {
  if (!existsSync(evidenceDir)) return []
  const files = (await fs.readdir(evidenceDir)).filter((file) => file.endsWith('-proof.json')).sort()
  return Promise.all(
    files.map(async (file) => {
      try {
        return { file, proof: JSON.parse(await fs.readFile(path.join(evidenceDir, file), 'utf8')) }
      } catch {
        return { file, proof: null }
      }
    }),
  )
}

// Returns the problems that stop a proof from counting; an empty list means it counts.
export function assessProof(proof, { requireCurrent = false, runId, evidenceDir, routing = loadRouting() } = {}) {
  if (!proof) return ['proof JSON is unreadable']
  if (proof.schemaVersion !== SCHEMA_VERSION) {
    return [`schemaVersion ${proof.schemaVersion ?? 'missing'} is not ${SCHEMA_VERSION}; re-run the proof`]
  }
  const problems = []
  if (proof.verdict !== verdictFor(proof.checks ?? [])) problems.push('verdict does not match its checks')
  if (proof.verdict !== 'pass') {
    const failing = (proof.checks ?? []).filter((check) => check.status !== 'pass' && check.required !== false)
    const reasons = failing.map((check) => `${check.id}${check.note ? `: ${check.note}` : ''}`).join('; ')
    problems.push(`verdict=${proof.verdict}${reasons ? ` (${reasons})` : ''}`)
  }
  if (runId && proof.runId !== runId) problems.push(`proof belongs to run ${proof.runId}, not ${runId}`)
  if (evidenceDir) {
    const missing = (proof.artifacts ?? []).filter((name) => path.basename(name) !== name || !existsSync(path.join(evidenceDir, name)))
    if (missing.length) problems.push(`missing artifacts: ${missing.join(', ')}`)
  }
  if (requireCurrent && proof.featureIds?.length) {
    const { sourceHash } = sourceFingerprint(proof.featureIds, routing)
    if (sourceHash !== proof.sourceHash) {
      problems.push(`stale: files routed to ${proof.featureIds.join(', ')} changed since ${String(proof.gitSha).slice(0, 8)}`)
    }
    if (!proof.launchSourceHash) {
      problems.push('not tied to a launched stack: record it after control-oceans-admin launch for this run')
    } else if (proof.launchSourceHash !== proof.sourceHash) {
      problems.push(`stale: files routed to ${proof.featureIds.join(', ')} changed after launch, so the stack ran older code; relaunch and re-run`)
    }
  }
  return problems
}

function detectHarness() {
  if (process.env.OCEANS_VERIFY_HARNESS) return process.env.OCEANS_VERIFY_HARNESS
  if (process.env.CLAUDECODE) return 'claude'
  if (process.env.CI) return 'ci'
  return 'unknown'
}

function git(args, input) {
  return execFileSync('git', args, { cwd: repoRoot, encoding: 'utf8', input, maxBuffer: 64 * 1024 * 1024 }).trim()
}
