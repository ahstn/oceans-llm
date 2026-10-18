import type { McpServerView } from '@/types/api'

/**
 * An active server whose last discovery failed or is waiting on credentials. The server registry
 * counts and filters these, and both the registry row and the server details mark them destructive.
 * Disabled servers never need attention, whatever their last discovery said.
 */
export function needsAttention(server: Pick<McpServerView, 'status' | 'last_discovery_status'>) {
  return (
    server.status === 'active' &&
    ['failed', 'auth_required'].includes(server.last_discovery_status ?? '')
  )
}

const discoveryLabels: Record<string, string> = {
  success: 'Discovered',
  failed: 'Discovery failed',
  auth_required: 'Authentication required',
  disabled: 'Discovery disabled',
}

/** A sentence-case label for a server's last discovery status, shared by the registry and details. */
export function discoveryLabel(status: string | null | undefined) {
  if (!status) return 'Not discovered'
  const fallback = status.replaceAll('_', ' ')
  return discoveryLabels[status] ?? fallback.charAt(0).toUpperCase() + fallback.slice(1)
}

/** The host an admin recognises a server by, or the raw URL when it doesn't parse. */
export function endpointHost(server: Pick<McpServerView, 'server_url'>) {
  try {
    return new URL(server.server_url).hostname
  } catch {
    return server.server_url
  }
}

/** A discovery refresh run from this page, before the reloaded server list catches up with it. */
export interface DiscoveryRefreshResult {
  /** The gateway's discovery status, or `request_failed` when the refresh request didn't complete. */
  status: string
  /** The gateway's stable reason for a `failed` discovery (`error_kind`). */
  errorKind?: string | null
  httpStatus?: number | null
  errorSummary: string | null
  toolCount: number
}

export interface DiscoveryFeedback {
  failed: boolean
  title: string
  /** What happened, then what to do next. */
  message: string
  /** The gateway's own summary, kept for diagnosis. */
  detail: string | null
  retry: boolean
}

type FailureContext = { host: string; timeout: string; httpStatus: number | null }

function httpFailureMessage({ host, httpStatus }: FailureContext) {
  const code = httpStatus ? ` (HTTP ${httpStatus})` : ''
  if (httpStatus === 401 || httpStatus === 403) {
    return `Reached ${host}, but it rejected the gateway's credentials${code}. Check the auth config, then Retry.`
  }
  if (httpStatus === 404 || httpStatus === 405) {
    return `Reached ${host}, but found no MCP endpoint at this URL${code}. Check the endpoint, then Retry.`
  }
  if (httpStatus === 408 || httpStatus === 429 || (httpStatus ?? 0) >= 500) {
    return `Reached ${host}, but it couldn't handle the request${code}. Wait a moment, then Retry.`
  }
  return `Reached ${host}, but it returned an HTTP error${code}. Check the endpoint and auth config, then Retry.`
}

// Keyed by the gateway's `error_kind`, so copy doesn't depend on the summary's wording.
const failedDiscoveryMessages: Record<string, (context: FailureContext) => string> = {
  transport: ({ host }) => `Couldn't reach ${host}. Check the endpoint, then Retry.`,
  timeout: ({ host, timeout }) =>
    `Timed out after ${timeout} waiting for ${host}. Check the endpoint or raise the timeout in Configuration, then Retry.`,
  http: httpFailureMessage,
  json_rpc: ({ host }) =>
    `${host} answered as an MCP server but refused to list its tools; its error is below. Check the server's access requirements, then Retry.`,
  invalid_response: ({ host }) =>
    `Reached ${host}, but it didn't respond as an MCP server. Check that the endpoint is a Streamable HTTP MCP URL, then Retry.`,
  invalid_url: () => `The endpoint isn't a valid URL. Fix it in Configuration, then Retry.`,
  invalid_header: () =>
    `The gateway couldn't build this server's auth header. Check the auth config, then Retry.`,
  invalid_auth_config: () =>
    `The gateway couldn't load this server's credentials. Check the auth config, then Retry.`,
  response_too_large: ({ host }) =>
    `${host} sent a tool list larger than the gateway accepts. Limit the tools it exposes, then Retry.`,
  invalid_tool_schema: ({ host }) =>
    `${host} sent a tool with an invalid schema; the tool is named below. Fix it on the server, then Retry.`,
}

function authRequiredMessage(authMode: string) {
  if (authMode === 'oauth_obo') {
    return 'This server uses OAuth on-behalf-of, so the gateway can only discover its tools if the upstream allows public discovery. If it does, set discovery_auth to none in the auth config, then Retry.'
  }
  return "This server uses per-user credentials, so the gateway can't discover its tools on its own. Switch to a gateway auth mode in Configuration, then Retry."
}

/**
 * Explains a refresh result in the server overview: what failed and what to do next, with the
 * gateway's summary kept underneath.
 */
export function describeDiscoveryResult(
  result: DiscoveryRefreshResult,
  server: Pick<McpServerView, 'server_url' | 'timeout_ms' | 'auth_mode'>,
): DiscoveryFeedback {
  const detail = result.errorSummary
  const failure = (title: string, message: string, retry = true) => ({
    failed: true,
    title,
    message,
    detail,
    retry,
  })

  switch (result.status) {
    case 'success':
      return {
        failed: false,
        title: 'Discovery refreshed',
        message: `Found ${result.toolCount} tool${result.toolCount === 1 ? '' : 's'}.`,
        detail: null,
        retry: false,
      }
    case 'request_failed':
      return failure(
        'Discovery failed',
        "The discovery request didn't complete. Retry, or check the gateway logs if it keeps failing.",
      )
    case 'auth_required':
      return failure(discoveryLabel(result.status), authRequiredMessage(server.auth_mode))
    case 'disabled':
      return failure(
        discoveryLabel(result.status),
        "This server is disabled, so the gateway doesn't run discovery for it. Enable it from More actions first.",
        false,
      )
    default: {
      const context = {
        host: endpointHost(server),
        timeout: formatTimeout(server.timeout_ms),
        httpStatus: result.httpStatus ?? null,
      }
      const explain = result.errorKind ? failedDiscoveryMessages[result.errorKind] : undefined
      const message =
        explain?.(context) ??
        `Discovery didn't complete for ${context.host}. Check the endpoint and auth config, then Retry.`
      return failure(discoveryLabel(result.status), message)
    }
  }
}

// Rounds before splitting into units, so 59,999 ms reads `1 min`, never `60 s` or `0 min 60 s`.
function formatTimeout(ms: number) {
  if (ms < 1000) return `${ms} ms`
  const seconds = Math.round(ms / 10) / 100
  if (seconds < 60) return `${seconds} s`
  const total = Math.round(ms / 1000)
  const minutes = Math.floor(total / 60)
  const rest = total % 60
  return rest ? `${minutes} min ${rest} s` : `${minutes} min`
}
