#!/usr/bin/env node
// record | evidence | report over the proof envelopes in one run's evidence directory.
// Called by control-oceans-admin, which supplies OCEANS_VERIFY_RUN_ID and OCEANS_VERIFY_EVIDENCE_DIR.
import { existsSync, readdirSync } from 'node:fs'
import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { changedPaths, defaultBase, planFor } from './plan-verification.mjs'
import { assessProof, buildEnvelope, currentSources, loadRouting, readProofs, STATUSES, verdictFor, writeProof } from './proof.mjs'

// Older evidence names map onto routing.json feature IDs.
const ALIASES = { observability: ['leaderboard', 'agent-harnesses'], 'live-llm': ['live-llm-requests'] }

export function parseCheck(spec, required) {
  const match = /^([a-z0-9][a-z0-9._-]*)=([a-z]+)(?::(.*))?$/.exec(spec)
  if (!match || !STATUSES.includes(match[2])) {
    throw new Error(`Check must look like id=${STATUSES.join('|')}[:note], got "${spec}"`)
  }
  const check = { id: match[1], status: match[2], required }
  if (match[3]) check.note = match[3].trim()
  return check
}

async function record(args, evidenceDir) {
  const [featureArg, ...rest] = args
  if (!featureArg || featureArg.startsWith('--')) throw new Error('Usage: record <feature-id>[,<id>] --check id=status[:note] ...')
  const featureIds = featureArg.split(',')
  const checks = []
  const artifacts = []
  let file = `${featureIds[0]}-proof.json`
  for (let i = 0; i < rest.length; i += 2) {
    const [flag, value] = [rest[i], rest[i + 1]]
    if (value === undefined) throw new Error(`${flag} needs a value`)
    if (flag === '--check') checks.push(parseCheck(value, true))
    else if (flag === '--optional') checks.push(parseCheck(value, false))
    else if (flag === '--artifact') {
      if (path.basename(value) !== value) throw new Error(`--artifact must be a file name in the evidence directory, got ${value}`)
      if (!existsSync(path.join(evidenceDir, value))) throw new Error(`Artifact ${value} is not in ${evidenceDir}`)
      artifacts.push(value)
    } else if (flag === '--file') {
      if (path.basename(value) !== value) throw new Error(`--file must be a file name in the evidence directory, got ${value}`)
      file = value
    }
    else throw new Error(`Unknown flag ${flag}`)
  }
  if (checks.length === 0) throw new Error('Record at least one --check')
  if (artifacts.length === 0) throw new Error('Record at least one --artifact (screenshot or ARIA snapshot) from the evidence directory')
  const envelope = buildEnvelope({ featureIds, checks, artifacts, method: 'manual' })
  await writeProof(evidenceDir, file, envelope)
  console.log(`${file}: ${envelope.verdict} (${checks.length} checks)`)
  return envelope.verdict === 'pass' ? 0 : 1
}

export function latestFor(featureId, proofs) {
  return proofs
    .filter(({ proof }) => proof?.featureIds?.includes(featureId))
    .sort((a, b) => String(b.proof.generatedAt).localeCompare(String(a.proof.generatedAt)))[0]
}

async function evidence(args, evidenceDir, runId) {
  const unknown = args.filter((arg) => arg.startsWith('--') && arg !== '--require-current')
  if (unknown.length) throw new Error(`Unknown evidence option ${unknown.join(', ')}; only --require-current is supported`)
  const requireCurrent = args.includes('--require-current')
  const targets = args.filter((arg) => !arg.startsWith('--')).flatMap((arg) => ALIASES[arg] ?? [arg])
  if (!existsSync(evidenceDir)) {
    console.error(`No evidence directory exists for run ${runId}.`)
    return 1
  }
  readdirSync(evidenceDir).sort().forEach((file) => console.log(path.join(evidenceDir, file)))
  const routing = loadRouting()
  const proofs = await readProofs(evidenceDir)
  const rows = targets.length
    ? targets.map((id) => ({ id, entry: latestFor(id, proofs) }))
    : proofs.map((entry) => ({ id: entry.proof?.featureIds?.join(',') ?? entry.file, entry }))
  if (rows.length === 0) {
    console.error('No *-proof.json files in this run.')
    return 1
  }
  let failed = 0
  for (const { id, entry } of rows) {
    const problems = entry ? assessProof(entry.proof, { requireCurrent, runId, evidenceDir, routing }) : ['no proof recorded']
    if (problems.length) failed += 1
    const where = entry ? ` (${entry.file})` : ''
    console.log(`${problems.length ? 'FAIL' : 'PASS'} ${id}${where}${problems.length ? `: ${problems.join('; ')}` : ''}`)
  }
  return failed ? 1 : 0
}

function summariseChecks(checks = []) {
  const passed = checks.filter((check) => check.status === 'pass').length
  const notes = checks
    .filter((check) => check.status !== 'pass')
    .map((check) => `${check.id}${check.required === false ? ' (optional)' : ''}: ${check.status}${check.note ? `, ${check.note}` : ''}`)
  return [`${passed}/${checks.length}`, ...notes].join('; ')
}

const ICONS = { pass: '✅ pass', fail: '❌ fail', blocked: '⛔ blocked' }

// A proof's own verdict shows only when nothing stops it from counting: stale
// proofs keep their verdict with a warning, any other problem marks it invalid.
function verdictCell(proof, problems) {
  const manual = proof.method === 'manual' ? ' (manual)' : ''
  const invalid = problems.filter((problem) => !problem.startsWith('stale') && !problem.startsWith('verdict='))
  if (invalid.length) return `❌ invalid${manual}: ${invalid.join('; ')}`
  const stale = problems.filter((problem) => problem.startsWith('stale'))
  return `${ICONS[proof.verdict] ?? proof.verdict}${manual}${stale.length ? ` ⚠ ${stale.join('; ')}` : ''}`
}

// One row per feature, from its newest proof (as `evidence` reads it), then any
// unreadable proof files so they stay visible.
export function reportRows(proofs, assess) {
  const ids = [...new Set(proofs.flatMap(({ proof }) => proof?.featureIds ?? []))]
  const latest = new Map()
  for (const id of ids) {
    const entry = latestFor(id, proofs)
    const row = latest.get(entry.file) ?? { ...entry, problems: assess(entry.proof), ids: [] }
    row.ids.push(id)
    latest.set(entry.file, row)
  }
  const unreadable = proofs
    .filter(({ proof }) => !proof?.featureIds)
    .map((entry) => ({ ...entry, problems: assess(entry.proof), ids: [] }))
  return [...latest.values(), ...unreadable]
}

export function renderReport({ runId, rows, plan }) {
  const head = rows.find(({ proof }) => proof?.schemaVersion)?.proof
  const lines = [
    `### Runtime verification: run ${runId}${head ? ` @ ${head.gitSha.slice(0, 8)}${head.dirty ? '+dirty' : ''} (${head.harness}, gateway ${head.gatewayVersion ?? 'unknown'})` : ''}`,
    '',
  ]
  if (rows.length) {
    lines.push('| Feature | Verdict | Checks | Evidence |', '|---|---|---|---|')
    for (const { file, proof, problems, ids } of rows) {
      if (!proof?.schemaVersion || !proof.featureIds) {
        lines.push(`| ${file} | ❌ unreadable | ${problems.join('; ').replaceAll('|', '/')} | ${file} |`)
        continue
      }
      const artifacts = proof.artifacts ?? []
      const shown = artifacts.slice(0, 3).join(', ')
      const more = artifacts.length > 3 ? ` +${artifacts.length - 3} more` : ''
      const evidenceCell = [file, shown].filter(Boolean).join(', ') + more
      const cells = [ids.join(', '), verdictCell(proof, problems), summariseChecks(proof.checks), evidenceCell]
      lines.push(`| ${cells.map((cell) => cell.replaceAll('|', '/')).join(' | ')} |`)
    }
  } else {
    lines.push('No proofs were recorded in this run.')
  }
  if (plan) {
    const passing = new Set(rows.filter((row) => row.problems.length === 0).flatMap((row) => row.ids))
    const missing = plan.proofs.filter((proof) => !passing.has(proof.id)).map((proof) => proof.id)
    lines.push('')
    if (plan.noRuntimeSurface) lines.push('**Plan:** SKIP, no runtime surface.')
    lines.push(`**Planned but not verified:** ${missing.length ? missing.join(', ') : 'none'}`)
    if (plan.unmapped.length) lines.push(`**Unmapped paths:** ${plan.unmapped.join(', ')}`)
    const reasons = [...new Set(plan.skipped.map((entry) => entry.reason))]
    if (reasons.length) lines.push(`**Skipped:** ${reasons.join(' · ')}`)
  }
  lines.push('', `Evidence: \`${process.env.OCEANS_VERIFY_EVIDENCE_DIR ?? ''}\``)
  return lines.join('\n')
}

async function report(args, evidenceDir, runId) {
  const planIndex = args.indexOf('--plan')
  const routing = loadRouting()
  let plan
  if (planIndex >= 0) {
    const baseArg = args[planIndex + 1]
    const base = baseArg && !baseArg.startsWith('--') ? baseArg : defaultBase(routing)
    plan = planFor(changedPaths(base).paths, routing)
  }
  const proofs = await readProofs(evidenceDir)
  const rows = reportRows(proofs, (proof) => assessProof(proof, { requireCurrent: true, runId, evidenceDir, routing }))
  console.log(renderReport({ runId, rows, plan }))
  return 0
}

// `launch` saves the routed source snapshot that later proofs compare against.
async function snapshotSources([file]) {
  if (!file) throw new Error('Usage: snapshot-sources <file>')
  await fs.writeFile(file, `${JSON.stringify(currentSources(), null, 2)}\n`)
  return 0
}

// Adds one check to an existing proof and recomputes its verdict; stack cleanup
// uses it to attach the Skills storage result to skills-proof.json.
async function addCheck([file, spec], evidenceDir) {
  if (!file || path.basename(file) !== file || !spec) throw new Error('Usage: add-check <name>-proof.json id=status[:note]')
  const target = path.join(evidenceDir, file)
  if (!existsSync(target)) throw new Error(`${file} is not in ${evidenceDir}`)
  const proof = JSON.parse(await fs.readFile(target, 'utf8'))
  const check = parseCheck(spec, true)
  proof.checks = [...(proof.checks ?? []).filter((existing) => existing.id !== check.id), check]
  proof.verdict = verdictFor(proof.checks)
  await writeProof(evidenceDir, file, proof)
  return 0
}

async function main([command, ...args]) {
  const runId = process.env.OCEANS_VERIFY_RUN_ID
  const evidenceDir = process.env.OCEANS_VERIFY_EVIDENCE_DIR
  if (!runId || !evidenceDir) throw new Error('Run through control-oceans-admin so the run ID and evidence directory are set.')
  if (command === 'snapshot-sources') return snapshotSources(args)
  if (command === 'add-check') return addCheck(args, evidenceDir)
  if (command === 'record') return record(args, evidenceDir)
  if (command === 'evidence') return evidence(args, evidenceDir, runId)
  if (command === 'report') return report(args, evidenceDir, runId)
  throw new Error(`Unknown proof command ${command}`)
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).then(
    (code) => {
      process.exitCode = code
    },
    (error) => {
      console.error(error.message)
      process.exitCode = 2
    },
  )
}
