// Runs tests/harness-integration (real OpenCode, Pi, OMP, Claude Code and the packed Oceans plugin
// against a temporary gateway built from this checkout) and records one proof check per test.
import { execFileSync, spawn } from 'node:child_process'
import { createWriteStream } from 'node:fs'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'

import { createProofRecorder, repoRoot } from './proof.mjs'

const RESULTS = 'harness-integration-results.json'
const LIVE_LINE =
  /^Live plugin pass: (?<family>\w+)\/(?<variant>\w+); (?<endpoint>\S+); request (?<requestLogId>\S+); tier (?<requested>\S+) -> (?<served>\S+); tokens (?<prompt>\d+)\/(?<completion>\d+); session \$(?<opencodeCost>[\d.e-]+); oceans \$(?<oceansCost>[\d.e-]+)$/

const evidenceDir = process.env.OCEANS_VERIFY_EVIDENCE_DIR
if (!evidenceDir) throw new Error('Run through control-oceans-admin so the evidence directory is set')
const suiteDir = path.join(repoRoot, 'tests/harness-integration')
const logFile = path.join(evidenceDir, '..', 'harness-integration.log')
const proof = createProofRecorder({
  evidenceDir,
  file: 'harness-integration-proof.json',
  featureIds: ['harness-integration'],
})
const details = { suite: 'tests/harness-integration', log: path.basename(logFile), live: [] }

const vertex = await vertexEnvironment()
try {
  const apiKey = process.env.OPENROUTER_API_KEY || fnoxValue('OPENROUTER_API_KEY')
  // Without Vertex settings the suite skips its Vertex cells, so record why.
  details.vertex = vertex.source
  if (apiKey) {
    // A suite that dies before writing its report must not leave an older report to read.
    await rm(path.join(evidenceDir, RESULTS), { force: true })
    const { exitCode, stdout } = await runSuite({ ...vertex.env, OPENROUTER_API_KEY: apiKey })
    details.exitCode = exitCode
    details.live = stdout.split('\n').flatMap(liveRequest)
    // A failed teardown or a signal (exit code null) can leave a report of passed tests.
    proof.add({
      id: 'harness-suite-exit',
      status: exitCode === 0 ? 'pass' : 'fail',
      observed: `exit ${exitCode}`,
      expected: 'exit 0',
    })
    const report = JSON.parse(await readFile(path.join(evidenceDir, RESULTS), 'utf8'))
    proof.artifact(RESULTS)
    recordTests(report)
  } else {
    proof.add({
      id: 'openrouter-credential',
      status: 'blocked',
      note: 'OPENROUTER_API_KEY is unset and fnox could not resolve it without a prompt',
    })
  }
  const envelope = await proof.write({ details })
  console.log(`harness integration proof: ${envelope.verdict}`)
  console.log(`evidence: ${evidenceDir}`)
  if (envelope.verdict !== 'pass') process.exitCode = 1
} catch (error) {
  await proof.write({ details, error })
  process.exitCode = 1
} finally {
  await vertex.cleanup()
}

/** A secret from fnox, or '' when fnox cannot resolve it without a prompt; never printed or saved. */
function fnoxValue(name) {
  try {
    return execFileSync('fnox', ['--non-interactive', '--no-daemon', 'get', name], {
      cwd: repoRoot,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim()
  } catch {
    return ''
  }
}

/**
 * Vertex AI settings for the suite: the caller's environment, else fnox. fnox holds the service
 * account JSON itself, so it goes to a 0600 file outside the repository and the evidence folder,
 * which `cleanup` removes. Values are never logged.
 */
async function vertexEnvironment() {
  const none = async () => {}
  if (process.env.OCEANS_HARNESS_VERTEX_PROJECT && process.env.GOOGLE_APPLICATION_CREDENTIALS) {
    return { source: 'environment', env: {}, cleanup: none }
  }
  const project = fnoxValue('OCEANS_HARNESS_VERTEX_PROJECT')
  const serviceAccount = project ? fnoxValue('OCEANS_HARNESS_VERTEX_SA_JSON') : ''
  if (!project || !serviceAccount) return { source: 'not configured', env: {}, cleanup: none }
  const directory = await mkdtemp(path.join(tmpdir(), 'oceans-vertex-'))
  const credentials = path.join(directory, 'service-account.json')
  await writeFile(credentials, serviceAccount, { mode: 0o600 })
  return {
    source: 'fnox',
    env: { OCEANS_HARNESS_VERTEX_PROJECT: project, GOOGLE_APPLICATION_CREDENTIALS: credentials },
    cleanup: () => rm(directory, { force: true, recursive: true }),
  }
}

/** Streams the suite to the terminal and the run log; resolves with its exit code and stdout. */
function runSuite(secrets) {
  const log = createWriteStream(logFile)
  const child = spawn(
    'npm',
    ['test', '--', '--reporter=verbose', '--reporter=json', `--outputFile.json=${path.join(evidenceDir, RESULTS)}`],
    { cwd: suiteDir, env: { ...process.env, ...secrets }, stdio: ['ignore', 'pipe', 'pipe'] },
  )
  let stdout = ''
  child.stdout.on('data', (chunk) => {
    stdout += chunk
    log.write(chunk)
    process.stdout.write(chunk)
  })
  child.stderr.on('data', (chunk) => {
    log.write(chunk)
    process.stderr.write(chunk)
  })
  return new Promise((resolve, reject) => {
    child.on('error', reject)
    child.on('close', (exitCode) => log.end(() => resolve({ exitCode, stdout })))
  })
}

function liveRequest(line) {
  const match = LIVE_LINE.exec(line.trim())
  if (!match) return []
  const { prompt, completion, opencodeCost, oceansCost, ...fields } = match.groups
  return [
    {
      ...fields,
      promptTokens: Number(prompt),
      completionTokens: Number(completion),
      opencodeCostUsd: Number(opencodeCost),
      oceansCostUsd: Number(oceansCost),
    },
  ]
}

function recordTests(report) {
  const liveCases = []
  for (const file of report.testResults) {
    const prefix = path.basename(file.name).replace(/\.test\.ts$/, '')
    for (const test of file.assertionResults) {
      const id = slug(`${prefix} ${[...test.ancestorTitles, test.title].join(' ')}`)
      if (prefix === 'opencode-plugin' && test.title.includes('records live usage')) liveCases.push(test)
      if (test.status === 'passed') proof.pass(id)
      else if (test.status === 'failed')
        proof.add({ id, status: 'fail', note: firstLine(test.failureMessages?.[0]) })
      else proof.add({ id, status: 'blocked', required: false, note: `${test.status} by the suite` })
    }
  }
  const live = details.live.length
  proof.add({
    id: 'plugin-live-usage',
    status: liveCases.length > 0 && live === liveCases.length ? 'pass' : 'fail',
    observed: `${live} live-request lines`,
    expected: `${liveCases.length} plugin live cases`,
  })
}

function slug(text) {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 120)
}

function firstLine(message = 'failed without a message') {
  return message.split('\n')[0].slice(0, 300)
}
