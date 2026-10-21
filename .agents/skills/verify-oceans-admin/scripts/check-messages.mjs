#!/usr/bin/env node
// Offline Messages regression gate. No gateway stack or upstream credential required.
import { spawnSync } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { assessResults, regressionFingerprint } from './messages-regressions.mjs'
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../../..')


function main(args) {
  if (args.length !== 2 || args[0] !== '--output' || !path.isAbsolute(args[1])) {
    throw new Error('Usage: mise exec -- node .agents/skills/verify-oceans-admin/scripts/check-messages.mjs --output /absolute/path/messages-regressions.json')
  }
  const outputFile = args[1]
  const source = regressionFingerprint()
  mkdirSync(path.dirname(outputFile), { recursive: true })
  const save = (result) => writeFileSync(outputFile, `${JSON.stringify({
    schemaVersion: 1, generatedAt: new Date().toISOString(), ...source,
    scope: 'offline Rust tests and local provider HTTP fixtures; not live native-provider or Claude Code proof',
    ...result,
  }, null, 2)}\n`)
  // Never leave an older passing result behind if compilation or execution is interrupted.
  save({ verdict: 'fail', checks: [{ id: 'runner-completed', status: 'fail' }] })
  const result = spawnSync(process.env.MISE_BIN || 'mise', [
    'exec', '--', 'cargo', 'test', '-p', 'gateway-core', '-p', 'gateway-providers',
    '-p', 'gateway', '--lib', '--', '--color', 'never',
  ], { cwd: repoRoot, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 })
  const assessment = assessResults(result.stdout || '', result.status)
  if (source.sourceHash !== regressionFingerprint().sourceHash) {
    assessment.verdict = 'fail'
    assessment.checks.push({ id: 'source-unchanged-during-tests', status: 'fail' })
  }
  save(assessment)
  console.log(`${assessment.verdict.toUpperCase()} Messages regressions: ${assessment.passedTests} tests; ${outputFile}`)
  if (assessment.verdict !== 'pass') {
    console.error(assessment.checks.filter((check) => check.status !== 'pass').map((check) => check.id).join(', '))
    console.error('Re-run mise exec -- cargo test -p gateway-core -p gateway-providers -p gateway --lib for diagnostics.')
    process.exitCode = 1
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main(process.argv.slice(2))
