import { execFile } from 'node:child_process'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
import { readReviewResult } from './result-artifact'
import type { EffectiveConfig, PullRequestContext, ReviewResult } from './types'

const executeFile = promisify(execFile)
export const actionRoot = fileURLToPath(new URL('../', import.meta.url))

export interface PiReviewRequest {
  context: PullRequestContext
  effectiveConfig: EffectiveConfig
  workspace: string
  oceansUrl: string
}

// Provider credentials are opt-in workflow environment variables. The GitHub and
// Oceans control-plane inputs must never be inherited by the review process.
const providerEnvironment = [
  'ANTHROPIC_API_KEY',
  'OPENAI_API_KEY',
  'OPENROUTER_API_KEY',
  'GOOGLE_API_KEY',
  'GEMINI_API_KEY',
  'EXA_API_KEY',
  'BRAVE_API_KEY',
  'TAVILY_API_KEY',
]

export function reviewEnvironment(tempDir: string, oceansApiKey: string): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {
    PATH: `${join(actionRoot, 'node_modules/.bin')}:${process.env.PATH ?? ''}`,
    HOME: tempDir,
    TMPDIR: tempDir,
    PI_CODING_AGENT_DIR: join(tempDir, 'agent'),
    PI_MCP_CONFIG_MODE: 'exclusive',
    PI_SKIP_VERSION_CHECK: '1',
    PI_TELEMETRY: '0',
    OCEANS_REVIEW_API_KEY: oceansApiKey,
  }
  for (const name of providerEnvironment) {
    if (process.env[name]) env[name] = process.env[name]
  }
  return env
}

export async function invokePi(
  request: PiReviewRequest,
  oceansApiKey: string,
  timeoutMinutes: number,
): Promise<ReviewResult> {
  const tempDir = mkdtempSync(join(tmpdir(), 'oceans-review-agent-'))
  const requestPath = join(tempDir, 'request.json')
  const resultPath = join(tempDir, 'result.json')
  writeFileSync(requestPath, JSON.stringify(request), { mode: 0o600 })
  try {
    await executeFile(
      process.execPath,
      [
        '--import',
        fileURLToPath(import.meta.resolve('tsx')),
        join(actionRoot, 'src/pi-worker.ts'),
        requestPath,
        resultPath,
      ],
      {
        cwd: actionRoot,
        env: reviewEnvironment(tempDir, oceansApiKey),
        timeout: timeoutMinutes * 60_000,
        maxBuffer: 1024 * 1024,
      },
    )
    return readReviewResult(resultPath)
  } catch (error) {
    const failure = error as Error & { stderr?: string; killed?: boolean }
    if (failure.killed)
      throw new Error(`Pi review exceeded ${timeoutMinutes} minutes`, { cause: error })
    // The worker emits only its terminal error, never prompts or response text.
    throw new Error(`Pi review failed: ${failure.stderr?.trim() || failure.message}`, {
      cause: error,
    })
  } finally {
    rmSync(tempDir, { recursive: true, force: true })
  }
}
