import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  createAgentSession,
  DefaultResourceLoader,
  ModelRuntime,
  SessionManager,
  SettingsManager,
  type ExtensionFactory,
  type ToolDefinition,
} from '@earendil-works/pi-coding-agent'
import { Type, type Static } from 'typebox'
import { registerSubagentCapabilityCeiling } from 'pi-subagents/capability-ceiling'
import type { PiReviewRequest } from './pi'
import { loadReviewDiff, type ReviewDiff } from './review-diff'

const root = fileURLToPath(new URL('../', import.meta.url))
export const reviewPackages = ['pi-mcp-adapter', 'pi-subagents', 'pi-web-access'] as const
const readTools = ['read', 'grep', 'find', 'ls']
const reviewTools = [
  ...readTools,
  'web_search',
  'fetch_content',
  'get_search_content',
  'mcp',
  'subagent',
  'submit_review',
]

const resultSchema = Type.Object({
  summary: Type.String({ maxLength: 60000 }),
  findings: Type.Array(
    Type.Object({
      path: Type.String({ minLength: 1 }),
      line: Type.Integer({ minimum: 1 }),
      severity: Type.Union([
        Type.Literal('low'),
        Type.Literal('medium'),
        Type.Literal('high'),
        Type.Literal('critical'),
      ]),
      message: Type.String({ minLength: 1, maxLength: 12000 }),
    }),
    { maxItems: 100 },
  ),
  degraded_features: Type.Array(Type.String(), { maxItems: 20 }),
})
type Submission = Static<typeof resultSchema>

function resultTool(
  diff: ReviewDiff,
  submit: (result: Submission) => void,
): ToolDefinition<typeof resultSchema> {
  return {
    name: 'submit_review',
    label: 'Submit review',
    description:
      'Submit the final review for validation and later GitHub publishing. Does not post to GitHub.',
    parameters: resultSchema,
    async execute(_id, result) {
      for (const finding of result.findings) {
        if (!diff.anchors.get(finding.path)?.has(finding.line)) {
          throw new Error(
            `Finding must reference a changed RIGHT-side line: ${finding.path}:${finding.line}`,
          )
        }
      }
      submit(result)
      return { content: [{ type: 'text', text: 'Review accepted.' }], details: {} }
    },
  }
}

const reviewPolicy: ExtensionFactory = (pi) => {
  pi.on('tool_call', async (event) => {
    if (event.toolName !== 'subagent') return
    // Keep the package's management, script, output-path, and worktree APIs out of CI reviews.
    const allowed = new Set(['agent', 'task', 'async', 'agentScope', 'context'])
    if (
      event.input.async !== false ||
      event.input.agentScope !== 'user' ||
      event.input.context !== 'fresh' ||
      Object.keys(event.input).some((key) => !allowed.has(key))
    ) {
      return {
        block: true,
        reason: 'Use only agent, task, async:false, agentScope:"user", and context:"fresh".',
      }
    }
  })
}

function writeRuntimeConfig(request: PiReviewRequest, agentDir: string): string {
  mkdirSync(agentDir, { recursive: true })
  copyFileSync(join(root, 'config/web-search.json'), join(agentDir, 'web-search.json'))
  mkdirSync(join(agentDir, 'extensions/subagent'), { recursive: true })
  writeFileSync(join(agentDir, 'mcp.json'), JSON.stringify({ mcpServers: {} }))
  writeFileSync(
    join(agentDir, 'extensions/subagent/config.json'),
    JSON.stringify({
      asyncByDefault: false,
      maxSubagentDepth: 1,
      maxSubagentSpawnsPerRun: 4,
    }),
  )
  const { effectiveConfig: config } = request
  if (config.model_execution_mode === 'direct') return String(config.model_id)
  const baseUrl = `${(config.oceans_base_url || request.oceansUrl).replace(/\/+$/, '').replace(/\/v1$/, '')}/v1`
  writeFileSync(
    join(agentDir, 'models.json'),
    JSON.stringify({
      providers: {
        oceans: {
          baseUrl,
          api: 'openai-completions',
          apiKey: '$OCEANS_REVIEW_API_KEY',
          models: [
            {
              id: config.model_id,
              name: config.model_id,
              reasoning: false,
              input: ['text'],
              contextWindow: 128000,
              maxTokens: 8192,
              cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
            },
          ],
        },
      },
    }),
    { mode: 0o600 },
  )
  return `oceans/${config.model_id}`
}

export async function runPiSession(request: PiReviewRequest, resultPath: string): Promise<void> {
  const agentDir = process.env.PI_CODING_AGENT_DIR
  if (!agentDir) throw new Error('PI_CODING_AGENT_DIR must point to an isolated run directory')
  const modelName = writeRuntimeConfig(request, agentDir)
  const separator = modelName.indexOf('/')
  if (separator < 1) throw new Error('Direct mode requires model-id in provider/model form')
  const runtime = await ModelRuntime.create({
    authPath: join(agentDir, 'auth.json'),
    modelsPath: join(agentDir, 'models.json'),
    allowModelNetwork: false,
  })
  const model = runtime.getModel(modelName.slice(0, separator), modelName.slice(separator + 1))
  if (!model) throw new Error(`Pi model is not configured: ${modelName}`)
  const diff = loadReviewDiff(request.workspace, request.context)
  const diffPath = join(dirname(resultPath), 'review.diff')
  writeFileSync(diffPath, diff.text)
  const settings = SettingsManager.inMemory({
    defaultProjectTrust: 'never',
    enableInstallTelemetry: false,
  })
  const loader = new DefaultResourceLoader({
    cwd: request.workspace,
    agentDir,
    settingsManager: settings,
    noExtensions: true,
    noSkills: true,
    noPromptTemplates: true,
    noThemes: true,
    noContextFiles: true,
    additionalExtensionPaths: reviewPackages.map((name) =>
      dirname(fileURLToPath(import.meta.resolve(name))),
    ),
    additionalSkillPaths: [join(root, 'skills')],
    extensionFactories: [reviewPolicy],
    systemPrompt: readFileSync(join(root, 'prompts/review.md'), 'utf8'),
  })
  await loader.reload()
  const errors = loader.getExtensions().errors
  if (errors.length)
    throw new Error(
      `Pi package loading failed: ${errors.map((e) => `${e.path}: ${e.error}`).join('; ')}`,
    )
  let submission: Submission | undefined
  const { session } = await createAgentSession({
    cwd: request.workspace,
    agentDir,
    modelRuntime: runtime,
    model,
    thinkingLevel: 'off',
    settingsManager: settings,
    resourceLoader: loader,
    sessionManager: SessionManager.inMemory(request.workspace),
    tools: reviewTools,
    customTools: [
      resultTool(diff, (result) => {
        if (submission) throw new Error('Review was already submitted')
        submission = result
      }),
    ],
  })
  const ceiling = registerSubagentCapabilityCeiling({
    sessionId: session.sessionId,
    source: 'oceans-review',
    ceiling: {
      allowedTools: readTools,
      allowedAgents: ['reviewer', 'scout', 'oracle'],
      denyExtensions: true,
    },
  })
  try {
    await session.bindExtensions({})
    await session.prompt(
      `Review this PR using the code-review skill.\n${JSON.stringify(request.context)}\nDiff path: ${diffPath}\nFeature settings: ${JSON.stringify(request.effectiveConfig)}`,
    )
    const lastAssistant = session.messages.at(-1)
    if (
      lastAssistant?.role === 'assistant' &&
      ['error', 'aborted'].includes(lastAssistant.stopReason)
    ) {
      throw new Error(
        `Pi model ${lastAssistant.stopReason}: ${lastAssistant.errorMessage || 'no provider error detail'}`,
      )
    }
    if (!submission) throw new Error('Pi finished without calling submit_review')
    writeFileSync(
      resultPath,
      JSON.stringify({ ...submission, metrics: { files_changed: diff.anchors.size } }),
    )
  } finally {
    ceiling.dispose()
    session.dispose()
  }
}
