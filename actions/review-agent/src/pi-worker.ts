import { readFileSync, writeSync } from 'node:fs'
import { join } from 'node:path'
import { runPiSession } from './pi-session'
import { workerErrorMarker, type PiReviewRequest } from './pi'

// pi-intercom starts a detached broker that idles until its last session
// disconnects. The sandbox PID namespace reaps it, but unsandboxed runs would
// leak one broker per review. The worker shares its PID namespace, so the PID
// file is valid in both modes.
function stopIntercomBroker() {
  try {
    const pidFile = join(process.env.PI_CODING_AGENT_DIR ?? '', 'intercom/broker.pid')
    const pid = Number(readFileSync(pidFile, 'utf8'))
    // 0 and negative PIDs would signal a whole process group.
    if (Number.isSafeInteger(pid) && pid > 1) process.kill(pid, 'SIGTERM')
  } catch {
    // No broker was started, or it has already exited.
  }
}

// Cancellation and the review timeout terminate the worker with SIGTERM.
process.once('SIGTERM', () => {
  stopIntercomBroker()
  process.exit(143)
})

try {
  const [requestPath, resultPath] = process.argv.slice(2)
  if (!requestPath || !resultPath) throw new Error('Expected request and result paths')
  const request = JSON.parse(readFileSync(requestPath, 'utf8')) as PiReviewRequest
  await runPiSession(request, resultPath)
  stopIntercomBroker()
  // Some package resource managers retain timers after session disposal. The
  // review is foreground-only; this process owns the full session lifetime.
  process.exit(0)
} catch (error) {
  let message = error instanceof Error ? error.message : String(error)
  for (const [name, value] of Object.entries(process.env)) {
    if (name.endsWith('_KEY') && value) message = message.replaceAll(value, '[REDACTED]')
  }
  writeSync(2, `${workerErrorMarker}${message}\n`)
  stopIntercomBroker()
  process.exit(1)
}
