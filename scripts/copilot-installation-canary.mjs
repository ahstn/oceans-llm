import { createSign, randomUUID } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { readFile, stat } from 'node:fs/promises'
import { pathToFileURL } from 'node:url'

const GITHUB_API_VERSION = '2022-11-28'
const COMPATIBILITY_PROFILE = Object.freeze(
  JSON.parse(
    readFileSync(new URL('./copilot-compatibility-profile.json', import.meta.url), 'utf8'),
  ),
)
const COPILOT_API_VERSION = COMPATIBILITY_PROFILE.api_version
const SERVER_TO_SERVER_DOC =
  'https://docs.github.com/en/copilot/how-tos/copilot-sdk/auth/server-to-server-tokens'
const BILLING_API_DOC =
  'https://docs.github.com/en/rest/billing/usage#get-billing-ai-credit-usage-report-for-an-organization'

const Status = Object.freeze({
  PASS: 'PASS',
  FAIL: 'FAIL',
  UNAVAILABLE: 'UNAVAILABLE',
})

export function sanitizeEvidence(value) {
  return String(value)
    .replace(/\bgh[pousr]_[A-Za-z0-9_]+\b/gu, '[REDACTED_GITHUB_TOKEN]')
    .replace(/\bgithub_pat_[A-Za-z0-9_]+\b/gu, '[REDACTED_GITHUB_TOKEN]')
    .replace(/\bBearer\s+[^\s,;]+/giu, 'Bearer [REDACTED]')
    .slice(0, 2_000)
}

function base64Url(value) {
  return Buffer.from(value).toString('base64url')
}

export function createAppJwt(appId, privateKey, nowSeconds = Math.floor(Date.now() / 1_000)) {
  const header = base64Url(JSON.stringify({ alg: 'RS256', typ: 'JWT' }))
  const claims = base64Url(
    JSON.stringify({ iat: nowSeconds - 60, exp: nowSeconds + 9 * 60, iss: appId }),
  )
  const unsignedToken = `${header}.${claims}`
  const signer = createSign('RSA-SHA256')
  signer.update(unsignedToken)
  signer.end()
  return `${unsignedToken}.${signer.sign(privateKey).toString('base64url')}`
}

function requiredEnvironment(environment, name) {
  const value = environment[name]?.trim()
  if (!value) {
    throw new Error(`${name} is required`)
  }
  return value
}

function optionalInteger(environment, name, defaultValue, maximum) {
  const raw = environment[name]?.trim()
  if (!raw) return defaultValue
  const value = Number(raw)
  if (!Number.isInteger(value) || value < 0 || value > maximum) {
    throw new Error(`${name} must be an integer from 0 through ${maximum}`)
  }
  return value
}

function safeInteger(value, name) {
  const parsed = Number(value)
  if (!Number.isSafeInteger(parsed) || parsed <= 0) {
    throw new Error(`${name} must be a positive safe integer`)
  }
  return parsed
}

async function loadConfiguration(environment) {
  const privateKeyPath = requiredEnvironment(environment, 'COPILOT_CANARY_PRIVATE_KEY_PATH')
  const keyStat = await stat(privateKeyPath)
  if (!keyStat.isFile()) {
    throw new Error('COPILOT_CANARY_PRIVATE_KEY_PATH must name a regular file')
  }

  return {
    appId: safeInteger(requiredEnvironment(environment, 'COPILOT_CANARY_APP_ID'), 'app ID'),
    installationId: safeInteger(
      requiredEnvironment(environment, 'COPILOT_CANARY_INSTALLATION_ID'),
      'installation ID',
    ),
    repositoryId: safeInteger(
      requiredEnvironment(environment, 'COPILOT_CANARY_REPOSITORY_ID'),
      'repository ID',
    ),
    expectedOwner: requiredEnvironment(environment, 'COPILOT_CANARY_EXPECTED_OWNER'),
    privateKey: await readFile(privateKeyPath, 'utf8'),
    privateKeyMode: keyStat.mode & 0o777,
    chatModel: environment.COPILOT_CANARY_CHAT_MODEL?.trim() || null,
    messagesModel: environment.COPILOT_CANARY_MESSAGES_MODEL?.trim() || null,
    billingToken: environment.COPILOT_CANARY_BILLING_TOKEN?.trim() || null,
    billingWaitSeconds: optionalInteger(
      environment,
      'COPILOT_CANARY_BILLING_WAIT_SECONDS',
      0,
      3_600,
    ),
    githubApiUrl: (environment.COPILOT_CANARY_GITHUB_API_URL || 'https://api.github.com').replace(
      /\/+$/u,
      '',
    ),
    copilotApiUrl: (environment.COPILOT_CANARY_API_URL || 'https://api.githubcopilot.com').replace(
      /\/+$/u,
      '',
    ),
    editorVersion:
      environment.COPILOT_CANARY_EDITOR_VERSION || COMPATIBILITY_PROFILE.editor_version,
    pluginVersion: COMPATIBILITY_PROFILE.plugin_version,
    integrationId:
      environment.COPILOT_CANARY_INTEGRATION_ID || COMPATIBILITY_PROFILE.integration_id,
  }
}

function createReport() {
  return {
    report_version: 1,
    started_at: new Date().toISOString(),
    checks: [],
  }
}

function addCheck(report, name, status, required, evidence) {
  report.checks.push({
    name,
    status,
    required,
    evidence: sanitizeEvidence(evidence),
  })
}

async function runCheck(report, name, required, action) {
  try {
    const outcome = await action()
    const isCheckedValue =
      outcome &&
      typeof outcome === 'object' &&
      Object.hasOwn(outcome, 'value') &&
      Object.hasOwn(outcome, 'evidence')
    addCheck(report, name, Status.PASS, required, isCheckedValue ? outcome.evidence : outcome)
    return isCheckedValue ? outcome.value : outcome
  } catch (error) {
    addCheck(report, name, required ? Status.FAIL : Status.UNAVAILABLE, required, error.message)
    return null
  }
}

async function runAvailabilityCheck(report, name, required, action) {
  try {
    const outcome = await action()
    addCheck(report, name, Status.PASS, required, outcome.evidence)
    return outcome.value
  } catch (error) {
    addCheck(report, name, Status.UNAVAILABLE, required, error.message)
    return null
  }
}

function checked(value, evidence) {
  return { value, evidence }
}

function unavailable(report, name, required, evidence) {
  addCheck(report, name, Status.UNAVAILABLE, required, evidence)
}

export function resultStatus(checks) {
  if (checks.some((check) => check.required && check.status === Status.FAIL)) return 'FAIL'
  if (checks.some((check) => check.required && check.status === Status.UNAVAILABLE)) {
    return 'INCOMPLETE'
  }
  return 'PASS'
}

async function responseBody(response) {
  const text = await response.text()
  if (!response.ok) {
    throw new Error(`HTTP ${response.status} ${response.statusText}: ${sanitizeEvidence(text)}`)
  }
  return text
}

async function requestJson(fetchImplementation, url, options) {
  const response = await fetchImplementation(url, options)
  const text = await responseBody(response)
  try {
    return { body: JSON.parse(text), headers: response.headers }
  } catch (error) {
    throw new Error(`Expected JSON from ${new URL(url).pathname}: ${error.message}`)
  }
}

async function requestText(fetchImplementation, url, options) {
  const response = await fetchImplementation(url, options)
  return {
    body: await responseBody(response),
    contentType: response.headers.get('content-type') || '',
  }
}

function githubHeaders(token) {
  return {
    accept: 'application/vnd.github+json',
    authorization: `Bearer ${token}`,
    'user-agent': 'oceans-llm-copilot-canary',
    'x-github-api-version': GITHUB_API_VERSION,
  }
}

function copilotHeaders(
  config,
  token,
  purpose = COMPATIBILITY_PROFILE.intent,
  initiator = 'user',
) {
  const requestId = randomUUID()
  return {
    accept: 'application/json',
    authorization: `Bearer ${token}`,
    'content-type': 'application/json',
    'copilot-integration-id': config.integrationId,
    'editor-version': config.editorVersion,
    'editor-plugin-version': config.pluginVersion,
    'openai-intent': purpose,
    'x-github-api-version': COPILOT_API_VERSION,
    'x-initiator': initiator,
    'x-interaction-type':
      purpose === 'model-access' ? 'model-access' : COMPATIBILITY_PROFILE.interaction_type,
    'x-request-id': requestId,
  }
}

async function getInstallation(fetchImplementation, config, appJwt) {
  const { body } = await requestJson(
    fetchImplementation,
    `${config.githubApiUrl}/app/installations/${config.installationId}`,
    { headers: githubHeaders(appJwt) },
  )
  return body
}

function validateInstallation(installation, config) {
  const owner = installation.account?.login
  const ownerType = installation.account?.type
  if (owner?.toLowerCase() !== config.expectedOwner.toLowerCase()) {
    throw new Error(`Installation owner ${owner || '<missing>'} did not match the expected owner`)
  }
  if (ownerType !== 'Organization') {
    throw new Error(`Installation owner type was ${ownerType || '<missing>'}, not Organization`)
  }
  if (installation.repository_selection !== 'all') {
    throw new Error('The installation does not have the required All repositories access')
  }
  if (installation.permissions?.copilot_requests !== 'write') {
    throw new Error('The installation does not have copilot_requests: write')
  }
  return `Organization ${owner}; All repositories; copilot_requests: write; ${SERVER_TO_SERVER_DOC}`
}

async function mintInstallationToken(fetchImplementation, config, appJwt, tokens) {
  const { body } = await requestJson(
    fetchImplementation,
    `${config.githubApiUrl}/app/installations/${config.installationId}/access_tokens`,
    {
      method: 'POST',
      headers: githubHeaders(appJwt),
      body: JSON.stringify({
        repository_ids: [config.repositoryId],
        permissions: { copilot_requests: 'write' },
      }),
    },
  )

  if (typeof body.token !== 'string' || !body.token.startsWith('ghs_')) {
    throw new Error('GitHub did not return a ghs_ installation token')
  }
  if (!tokens.includes(body.token)) tokens.push(body.token)
  if (body.permissions?.copilot_requests !== 'write') {
    throw new Error('The minted token response did not confirm copilot_requests: write')
  }
  if (
    !Array.isArray(body.repositories) ||
    body.repositories.length !== 1 ||
    body.repositories[0]?.id !== config.repositoryId
  ) {
    throw new Error('The minted token response did not confirm only the requested repository ID')
  }
  const expiresAt = Date.parse(body.expires_at)
  const lifetimeMinutes = (expiresAt - Date.now()) / 60_000
  if (!Number.isFinite(lifetimeMinutes) || lifetimeMinutes < 50 || lifetimeMinutes > 65) {
    throw new Error(
      `The minted token lifetime was ${lifetimeMinutes.toFixed(1)} minutes, not approximately one hour`,
    )
  }
  return { token: body.token, expiresAt }
}

async function revokeInstallationToken(fetchImplementation, config, token) {
  const response = await fetchImplementation(`${config.githubApiUrl}/installation/token`, {
    method: 'DELETE',
    headers: githubHeaders(token),
  })
  if (response.status !== 204) {
    throw new Error(`GitHub returned HTTP ${response.status} while revoking a canary token`)
  }
}

function normalizeEndpoint(endpoint) {
  return endpoint.replace(/^\/+/u, '').replace(/^v1\//u, '')
}

function supportsEndpoint(model, endpoint) {
  return (model.supported_endpoints || []).some(
    (candidate) => normalizeEndpoint(candidate) === normalizeEndpoint(endpoint),
  )
}

export function selectModel(models, configuredModel, endpoint, requirements = {}) {
  if (!configuredModel) return null
  const model = models.find((candidate) => candidate.id === configuredModel)
  if (!model) throw new Error(`Configured model ${configuredModel} was absent from /models`)
  if (model.policy?.state === 'disabled') {
    throw new Error(`Configured model ${configuredModel} is disabled by policy`)
  }
  if (!supportsEndpoint(model, endpoint)) {
    throw new Error(`Configured model ${configuredModel} does not advertise /${endpoint}`)
  }
  if (requirements.tools && model.capabilities?.supports?.tool_calls !== true) {
    throw new Error(`Configured model ${configuredModel} does not advertise tool calls`)
  }
  if (requirements.streaming && model.capabilities?.supports?.streaming !== true) {
    throw new Error(`Configured model ${configuredModel} does not advertise streaming`)
  }
  return model
}

async function getModels(fetchImplementation, config, token) {
  const { body } = await requestJson(fetchImplementation, `${config.copilotApiUrl}/models`, {
    headers: copilotHeaders(config, token, 'model-access'),
  })
  if (!Array.isArray(body.data) || body.data.length === 0) {
    throw new Error('/models returned no models')
  }
  return body.data
}

function postJson(fetchImplementation, config, token, path, payload, extraHeaders = {}) {
  return requestJson(fetchImplementation, `${config.copilotApiUrl}/${path}`, {
    method: 'POST',
    headers: { ...copilotHeaders(config, token), ...extraHeaders },
    body: JSON.stringify(payload),
  })
}

async function checkChat(fetchImplementation, config, token, modelId) {
  const { body } = await postJson(fetchImplementation, config, token, 'chat/completions', {
    model: modelId,
    messages: [{ role: 'user', content: 'Reply with exactly CANARY_OK.' }],
    max_tokens: 32,
    stream: false,
  })
  const content = body.choices?.[0]?.message?.content
  if (typeof content !== 'string' || content.length === 0) {
    throw new Error('/chat/completions returned no assistant text')
  }
  return `HTTP 2xx with assistant text from ${modelId}`
}

async function checkMessages(fetchImplementation, config, token, modelId) {
  const { body } = await postJson(
    fetchImplementation,
    config,
    token,
    'v1/messages',
    {
      model: modelId,
      messages: [{ role: 'user', content: 'Reply with exactly CANARY_OK.' }],
      max_tokens: 32,
      stream: false,
    },
    { 'anthropic-version': COMPATIBILITY_PROFILE.anthropic_version },
  )
  const hasText = body.content?.some(
    (block) => block.type === 'text' && typeof block.text === 'string' && block.text.length > 0,
  )
  if (!hasText) throw new Error('/v1/messages returned no assistant text block')
  return `HTTP 2xx with an assistant text block from ${modelId}`
}

export function parseSse(text) {
  const events = []
  let done = false
  for (const block of text.split(/\r?\n\r?\n/u)) {
    const data = block
      .split(/\r?\n/u)
      .filter((line) => line.startsWith('data:'))
      .map((line) => line.slice(5).trim())
      .join('\n')
    if (!data) continue
    if (data === '[DONE]') {
      done = true
      continue
    }
    try {
      events.push(JSON.parse(data))
    } catch (error) {
      throw new Error(`Invalid SSE JSON event: ${error.message}`)
    }
  }
  return { events, done }
}

async function checkStream(fetchImplementation, config, token, path, payload, kind) {
  const response = await requestText(fetchImplementation, `${config.copilotApiUrl}/${path}`, {
    method: 'POST',
    headers: {
      ...copilotHeaders(config, token),
      ...(kind === 'messages'
        ? { 'anthropic-version': COMPATIBILITY_PROFILE.anthropic_version }
        : {}),
    },
    body: JSON.stringify({ ...payload, stream: true }),
  })
  if (!response.contentType.includes('text/event-stream')) {
    throw new Error(`${path} did not return text/event-stream`)
  }
  const parsed = parseSse(response.body)
  validateStreamResult(parsed, path, kind)
  return `HTTP 2xx text/event-stream with ${parsed.events.length} JSON events`
}

export function validateStreamResult(parsed, path, kind) {
  if (parsed.events.length === 0) throw new Error(`${path} returned no SSE JSON events`)
  if (parsed.events.some((event) => event.error || event.type === 'error')) {
    throw new Error(`${path} returned an error event`)
  }
  const hasAssistantDelta = parsed.events.some((event) => {
    if (kind === 'chat') {
      return event.choices?.some(
        (choice) =>
          (typeof choice.delta?.content === 'string' && choice.delta.content.length > 0) ||
          choice.delta?.tool_calls?.length > 0,
      )
    }
    return (
      (event.type === 'content_block_delta' &&
        ((typeof event.delta?.text === 'string' && event.delta.text.length > 0) ||
          (typeof event.delta?.partial_json === 'string' &&
            event.delta.partial_json.length > 0))) ||
      (event.type === 'content_block_start' && event.content_block?.type === 'tool_use')
    )
  })
  if (!hasAssistantDelta) throw new Error(`${path} returned no assistant content delta`)
  if (kind === 'chat' && !parsed.done) throw new Error(`${path} did not return [DONE]`)
  if (kind === 'messages' && !parsed.events.some((event) => event.type === 'message_stop')) {
    throw new Error(`${path} did not return message_stop`)
  }
}

async function checkTools(fetchImplementation, config, token, modelId) {
  const toolName = 'get_canary_value'
  const { body } = await postJson(fetchImplementation, config, token, 'chat/completions', {
    model: modelId,
    messages: [{ role: 'user', content: 'Call the supplied tool.' }],
    max_tokens: 64,
    tools: [
      {
        type: 'function',
        function: {
          name: toolName,
          description: 'Return the fixed canary value.',
          parameters: { type: 'object', properties: {}, additionalProperties: false },
        },
      },
    ],
    tool_choice: { type: 'function', function: { name: toolName } },
  })
  const calls = body.choices?.[0]?.message?.tool_calls
  if (!calls?.some((call) => call.function?.name === toolName)) {
    throw new Error('/chat/completions did not return the forced tool call')
  }
  const selectedCall = calls.find((call) => call.function?.name === toolName)
  if (typeof selectedCall.id !== 'string' || selectedCall.id.length === 0) {
    throw new Error('/chat/completions returned a tool call without an ID')
  }

  const continuation = await postJson(
    fetchImplementation,
    config,
    token,
    'chat/completions',
    {
      model: modelId,
      messages: [
        { role: 'user', content: 'Call the supplied tool.' },
        { role: 'assistant', content: null, tool_calls: calls },
        { role: 'tool', tool_call_id: selectedCall.id, content: '{"value":"CANARY_OK"}' },
      ],
      max_tokens: 32,
    },
    { 'x-initiator': 'agent' },
  )
  if (!continuation.body.choices?.[0]?.message) {
    throw new Error('The agent-initiated tool-result continuation returned no assistant message')
  }
  return `HTTP 2xx for the forced ${toolName} call and agent-initiated tool-result continuation from ${modelId}`
}

function utcDayQuery() {
  const now = new Date()
  return new URLSearchParams({
    year: String(now.getUTCFullYear()),
    month: String(now.getUTCMonth() + 1),
    day: String(now.getUTCDate()),
  })
}

export function summarizeBilling(body) {
  const items = Array.isArray(body.usageItems) ? body.usageItems : []
  return items.reduce(
    (summary, item) => ({
      items: summary.items + 1,
      netQuantity: summary.netQuantity + (Number(item.netQuantity) || 0),
      netAmount: summary.netAmount + (Number(item.netAmount) || 0),
    }),
    { items: 0, netQuantity: 0, netAmount: 0 },
  )
}

async function getBillingSnapshot(fetchImplementation, config) {
  const query = utcDayQuery()
  const url = `${config.githubApiUrl}/organizations/${encodeURIComponent(config.expectedOwner)}/settings/billing/ai_credit/usage?${query}`
  const { body } = await requestJson(fetchImplementation, url, {
    headers: githubHeaders(config.billingToken),
  })
  if (body.organization?.toLowerCase() !== config.expectedOwner.toLowerCase()) {
    throw new Error('The billing response did not identify the expected organization')
  }
  return summarizeBilling(body)
}

function sleep(seconds) {
  return new Promise((resolve) => setTimeout(resolve, seconds * 1_000))
}

function modelInventory(models) {
  return models
    .filter((model) => model.policy?.state !== 'disabled')
    .map((model) => {
      const supports = model.capabilities?.supports || {}
      return {
        id: model.id,
        supported_endpoints: model.supported_endpoints || [],
        supports: {
          streaming: supports.streaming === true,
          tool_calls: supports.tool_calls === true,
          vision: supports.vision === true,
          structured_outputs: supports.structured_outputs === true,
        },
      }
    })
}

function printReport(report, output) {
  report.finished_at = new Date().toISOString()
  report.result = resultStatus(report.checks)
  output.write(`${JSON.stringify(report, null, 2)}\n`)
  return report.result === 'PASS' ? 0 : report.result === 'INCOMPLETE' ? 2 : 1
}

async function verifyInstallation(fetchImplementation, config, report, appJwt) {
  return runCheck(report, 'installation_owner_and_scope', true, async () => {
    const installation = await getInstallation(fetchImplementation, config, appJwt)
    return checked(installation, validateInstallation(installation, config))
  })
}

async function mintInitialToken(fetchImplementation, config, report, appJwt, tokens) {
  return runCheck(report, 'initial_installation_token', true, async () => {
    const token = await mintInstallationToken(fetchImplementation, config, appJwt, tokens)
    return checked(
      token,
      `Minted a repository-scoped ghs_ token that expires at ${new Date(token.expiresAt).toISOString()}`,
    )
  })
}

async function captureBillingBaseline(fetchImplementation, config, report) {
  if (!config.billingToken) {
    unavailable(
      report,
      'billing_api_baseline',
      true,
      'Set COPILOT_CANARY_BILLING_TOKEN to read the organization AI-credit aggregate',
    )
    return null
  }
  return runAvailabilityCheck(report, 'billing_api_baseline', true, async () => {
    const summary = await getBillingSnapshot(fetchImplementation, config)
    return checked(
      summary,
      `Daily organization aggregate was available before the canary: ${JSON.stringify(summary)}; ${BILLING_API_DOC}`,
    )
  })
}

async function loadModels(fetchImplementation, config, report, token) {
  if (!token) {
    unavailable(report, 'models', true, 'Initial installation token was not available')
    return null
  }
  return runCheck(report, 'models', true, async () => {
    const models = await getModels(fetchImplementation, config, token)
    return checked(
      models,
      `${models.length} models returned; inventory=${JSON.stringify(modelInventory(models))}`,
    )
  })
}

async function selectChatModel(models, config, report) {
  if (!config.chatModel) {
    unavailable(
      report,
      'chat_model_contract',
      true,
      'Set COPILOT_CANARY_CHAT_MODEL to a tool-capable chat model from the reported inventory',
    )
    return null
  }
  return runCheck(report, 'chat_model_contract', true, () => {
    const model = selectModel(models, config.chatModel, 'chat/completions', {
      tools: true,
      streaming: true,
    })
    return checked(model, `${model.id} advertises chat, streaming, and tool-call support`)
  })
}

async function selectMessagesModel(models, config, report) {
  if (!config.messagesModel) {
    unavailable(
      report,
      'messages_model_contract',
      true,
      'Set COPILOT_CANARY_MESSAGES_MODEL to a Messages model from the reported inventory',
    )
    return null
  }
  return runCheck(report, 'messages_model_contract', true, () => {
    const model = selectModel(models, config.messagesModel, 'v1/messages', { streaming: true })
    return checked(model, `${model.id} advertises /v1/messages and streaming`)
  })
}

async function runChatChecks(fetchImplementation, config, report, token, model) {
  if (!token || !model) {
    for (const name of ['chat_completions', 'chat_completions_stream', 'tools']) {
      unavailable(report, name, true, 'A verified chat model and installation token were not available')
    }
    return
  }
  await runCheck(report, 'chat_completions', true, () =>
    checkChat(fetchImplementation, config, token, model.id),
  )
  await runCheck(report, 'chat_completions_stream', true, () =>
    checkStream(
      fetchImplementation,
      config,
      token,
      'chat/completions',
      {
        model: model.id,
        messages: [{ role: 'user', content: 'Reply with CANARY_STREAM_OK.' }],
        max_tokens: 32,
      },
      'chat',
    ),
  )
  await runCheck(report, 'tools', true, () =>
    checkTools(fetchImplementation, config, token, model.id),
  )
}

async function runMessagesChecks(fetchImplementation, config, report, token, model) {
  if (!token || !model) {
    for (const name of ['messages', 'messages_stream']) {
      unavailable(report, name, true, 'A verified Messages model and installation token were not available')
    }
    return
  }
  await runCheck(report, 'messages', true, () =>
    checkMessages(fetchImplementation, config, token, model.id),
  )
  await runCheck(report, 'messages_stream', true, () =>
    checkStream(
      fetchImplementation,
      config,
      token,
      'v1/messages',
      {
        model: model.id,
        messages: [{ role: 'user', content: 'Reply with CANARY_STREAM_OK.' }],
        max_tokens: 32,
      },
      'messages',
    ),
  )
}

async function verifyRefresh(fetchImplementation, config, report, appJwt, firstToken, tokens) {
  if (!firstToken) {
    unavailable(report, 'token_refresh', true, 'The initial installation token was not available')
    return
  }
  await runCheck(report, 'token_refresh', true, async () => {
    const refreshed = await mintInstallationToken(fetchImplementation, config, appJwt, tokens)
    if (refreshed.token === firstToken) throw new Error('The refreshed token was not distinct')
    const models = await getModels(fetchImplementation, config, refreshed.token)
    return `A distinct refreshed ghs_ token authenticated to /models and returned ${models.length} models`
  })
}

async function verifyBilling(fetchImplementation, config, report, installation, before) {
  addCheck(
    report,
    'billing_owner_contract',
    installation ? Status.PASS : Status.UNAVAILABLE,
    true,
    installation
      ? `GitHub documents attribution to the installation owner, verified as ${config.expectedOwner}; ${SERVER_TO_SERVER_DOC}`
      : 'The installation owner could not be verified',
  )
  if (!before) {
    unavailable(report, 'billing_usage_delta', true, 'No readable billing baseline was available')
  } else {
    if (config.billingWaitSeconds > 0) await sleep(config.billingWaitSeconds)
    const after = await runAvailabilityCheck(report, 'billing_api_after', true, async () => {
      const summary = await getBillingSnapshot(fetchImplementation, config)
      return checked(summary, `Daily organization aggregate after the canary: ${JSON.stringify(summary)}`)
    })
    if (after) {
      const quantityDelta = after.netQuantity - before.netQuantity
      const amountDelta = after.netAmount - before.netAmount
      const changed = quantityDelta > 0 || amountDelta > 0
      addCheck(
        report,
        'billing_usage_delta',
        changed ? Status.PASS : Status.UNAVAILABLE,
        true,
        changed
          ? `The daily aggregate increased: netQuantity delta=${quantityDelta}, netAmount delta=${amountDelta}`
          : 'No aggregate increase was visible; GitHub billing data can lag the canary',
      )
    } else {
      unavailable(report, 'billing_usage_delta', true, 'The post-canary billing aggregate was unavailable')
    }
  }
  unavailable(
    report,
    'billing_request_attribution',
    false,
    'The public billing API is daily aggregate data without installation or request IDs; it cannot attribute this request in isolation',
  )
}

async function cleanupTokens(fetchImplementation, config, report, tokens) {
  if (tokens.length === 0) {
    unavailable(report, 'token_cleanup', false, 'No canary installation token was minted')
    return
  }
  const results = await Promise.allSettled(
    tokens.map((token) => revokeInstallationToken(fetchImplementation, config, token)),
  )
  const failures = results.filter((result) => result.status === 'rejected')
  addCheck(
    report,
    'token_cleanup',
    failures.length === 0 ? Status.PASS : Status.FAIL,
    true,
    failures.length === 0
      ? `Revoked ${tokens.length} in-memory canary installation token(s)`
      : `${failures.length} canary installation token(s) could not be revoked`,
  )
}

async function executeCanary(fetchImplementation, config, report, tokens) {
  addCheck(
    report,
    'compatibility_profile',
    Status.PASS,
    true,
    `${COMPATIBILITY_PROFILE.name}; editor=${config.editorVersion}; plugin=${config.pluginVersion}; integration=${config.integrationId}; intent=${COMPATIBILITY_PROFILE.intent}; interaction=${COMPATIBILITY_PROFILE.interaction_type}`,
  )
  const appJwt = createAppJwt(config.appId, config.privateKey)
  const installation = await verifyInstallation(fetchImplementation, config, report, appJwt)
  const initialToken = await mintInitialToken(fetchImplementation, config, report, appJwt, tokens)
  const billingBefore = await captureBillingBaseline(fetchImplementation, config, report)
  const models = await loadModels(fetchImplementation, config, report, initialToken?.token)
  const chatModel = models ? await selectChatModel(models, config, report) : null
  const messagesModel = models ? await selectMessagesModel(models, config, report) : null
  await runChatChecks(fetchImplementation, config, report, initialToken?.token, chatModel)
  await runMessagesChecks(fetchImplementation, config, report, initialToken?.token, messagesModel)
  await verifyRefresh(
    fetchImplementation,
    config,
    report,
    appJwt,
    initialToken?.token,
    tokens,
  )
  await verifyBilling(fetchImplementation, config, report, installation, billingBefore)
}

export async function runCanary(
  environment = process.env,
  fetchImplementation = fetch,
  output = process.stdout,
) {
  const report = createReport()
  let config
  try {
    config = await loadConfiguration(environment)
  } catch (error) {
    addCheck(report, 'configuration', Status.FAIL, true, error.message)
    return printReport(report, output)
  }

  const insecureKey = Boolean(config.privateKeyMode & 0o077)
  addCheck(
    report,
    'private_key_file_permissions',
    insecureKey ? Status.FAIL : Status.PASS,
    true,
    `Private key mode is 0${config.privateKeyMode.toString(8)}; group and other access must be absent`,
  )
  if (insecureKey) return printReport(report, output)

  const tokens = []
  try {
    await executeCanary(fetchImplementation, config, report, tokens)
  } catch (error) {
    addCheck(report, 'canary_workflow', Status.FAIL, true, error.message)
  } finally {
    await cleanupTokens(fetchImplementation, config, report, tokens)
  }
  return printReport(report, output)
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = await runCanary()
}
