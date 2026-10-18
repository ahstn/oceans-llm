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

type FailureContext = { host: string; timeout: string; status: string }

// Summaries begin with the gateway's error text (gateway-mcp `McpClientError` and the discovery
// service). Anything unrecognised falls back to a general next step, with the summary still shown.
const failedDiscoveryMessages: [RegExp, (context: FailureContext) => string][] = [
  [
    /^MCP transport failure/,
    ({ host }) => `Couldn't reach ${host}. Check the endpoint, then Retry.`,
  ],
  [
    /^MCP upstream timed out/,
    ({ host, timeout }) =>
      `Timed out after ${timeout} waiting for ${host}. Check the endpoint or raise the timeout in Configuration, then Retry.`,
  ],
  [
    /^MCP upstream returned HTTP 40[13]\b/,
    ({ host, status }) =>
      `Reached ${host}, but it rejected the gateway's credentials (HTTP ${status}). Check the auth config, then Retry.`,
  ],
  [
    /^MCP upstream returned HTTP 40[45]\b/,
    ({ host, status }) =>
      `Reached ${host}, but found no MCP endpoint at this URL (HTTP ${status}). Check the endpoint, then Retry.`,
  ],
  [
    /^MCP upstream returned HTTP (408|429|5\d\d)\b/,
    ({ host, status }) =>
      `Reached ${host}, but it couldn't handle the request (HTTP ${status}). Wait a moment, then Retry.`,
  ],
  [
    /^MCP upstream returned HTTP/,
    ({ host, status }) =>
      `Reached ${host}, but it returned HTTP ${status}. Check the endpoint and auth config, then Retry.`,
  ],
  [
    /^(MCP JSON-RPC error|invalid MCP response)/,
    ({ host }) =>
      `Reached ${host}, but it didn't respond as an MCP server. Check that the endpoint is a Streamable HTTP MCP URL, then Retry.`,
  ],
  [
    /^invalid MCP server URL/,
    () => `The endpoint isn't a valid URL. Fix it in Configuration, then Retry.`,
  ],
  [
    /secret_ref|auth_config\./,
    () => `The gateway couldn't load this server's credentials. Check the auth config, then Retry.`,
  ],
]

/**
 * Explains a refresh result in the server overview: what failed and what to do next, with the
 * gateway's summary kept underneath.
 */
export function describeDiscoveryResult(
  result: DiscoveryRefreshResult,
  server: Pick<McpServerView, 'server_url' | 'timeout_ms'>,
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
      return failure(
        discoveryLabel(result.status),
        "This server uses per-user credentials, so the gateway can't discover its tools on its own. Switch to a gateway auth mode in Configuration, then Retry.",
      )
    case 'disabled':
      return failure(
        discoveryLabel(result.status),
        "This server is disabled, so the gateway doesn't run discovery for it. Enable it from More actions first.",
        false,
      )
    default: {
      const summary = detail ?? ''
      const context = {
        host: endpointHost(server),
        timeout: formatTimeout(server.timeout_ms),
        status: /HTTP (\d{3})/.exec(summary)?.[1] ?? '',
      }
      const message =
        failedDiscoveryMessages.find(([pattern]) => pattern.test(summary))?.[1](context) ??
        `Discovery didn't complete for ${context.host}. Check the endpoint and auth config, then Retry.`
      return failure(discoveryLabel(result.status), message)
    }
  }
}

function formatTimeout(ms: number) {
  if (ms < 1000) return `${ms} ms`
  if (ms < 60_000) return `${Number((ms / 1000).toFixed(2))} s`
  const minutes = Math.floor(ms / 60_000)
  const seconds = Math.round((ms % 60_000) / 1000)
  return seconds ? `${minutes} min ${seconds} s` : `${minutes} min`
}
