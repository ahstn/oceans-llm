import { readFileSync, writeSync } from 'node:fs'
import { runPiSession } from './pi-session'
import type { PiReviewRequest } from './pi'

try {
  const [requestPath, resultPath] = process.argv.slice(2)
  if (!requestPath || !resultPath) throw new Error('Expected request and result paths')
  const request = JSON.parse(readFileSync(requestPath, 'utf8')) as PiReviewRequest
  await runPiSession(request, resultPath)
  // Some package resource managers retain timers after session disposal. The
  // review is foreground-only; this process owns the full session lifetime.
  process.exit(0)
} catch (error) {
  let message = error instanceof Error ? error.message : String(error)
  for (const [name, value] of Object.entries(process.env)) {
    if (name.endsWith('_KEY') && value) message = message.replaceAll(value, '[REDACTED]')
  }
  writeSync(2, `${message}\n`)
  process.exit(1)
}
