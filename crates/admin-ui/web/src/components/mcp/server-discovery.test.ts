import { describe, expect, it } from 'vitest'

import { describeDiscoveryResult } from './server-discovery'

const server = { server_url: 'https://unreachable.invalid/mcp', timeout_ms: 30_000 }

describe('describeDiscoveryResult', () => {
  it.each([
    [
      'MCP transport failure: error sending request',
      "Couldn't reach unreachable.invalid. Check the endpoint, then Retry.",
    ],
    [
      'MCP upstream timed out',
      'Timed out after 30 s waiting for unreachable.invalid. Check the endpoint or raise the timeout in Configuration, then Retry.',
    ],
    [
      'MCP upstream returned HTTP 401',
      "Reached unreachable.invalid, but it rejected the gateway's credentials (HTTP 401). Check the auth config, then Retry.",
    ],
    [
      'MCP upstream returned HTTP 404',
      'Reached unreachable.invalid, but found no MCP endpoint at this URL (HTTP 404). Check the endpoint, then Retry.',
    ],
    [
      'MCP upstream returned HTTP 503',
      "Reached unreachable.invalid, but it couldn't handle the request (HTTP 503). Wait a moment, then Retry.",
    ],
    [
      'MCP upstream returned HTTP 418',
      'Reached unreachable.invalid, but it returned HTTP 418. Check the endpoint and auth config, then Retry.',
    ],
    [
      'MCP JSON-RPC error -32601',
      "Reached unreachable.invalid, but it didn't respond as an MCP server. Check that the endpoint is a Streamable HTTP MCP URL, then Retry.",
    ],
    [
      'request validation failed: secret_ref env/OCEANS_MCP_DISCOVERY_GITHUB is not available for MCP use',
      "The gateway couldn't load this server's credentials. Check the auth config, then Retry.",
    ],
    [
      'an error the UI has not seen before',
      "Discovery didn't complete for unreachable.invalid. Check the endpoint and auth config, then Retry.",
    ],
  ])('explains "%s" and keeps it as the detail', (errorSummary, message) => {
    expect(
      describeDiscoveryResult({ status: 'failed', errorSummary, toolCount: 0 }, server),
    ).toEqual({
      failed: true,
      title: 'Discovery failed',
      message,
      detail: errorSummary,
      retry: true,
    })
  })

  it('names the next step for failures before the upstream is reached', () => {
    const explain = (status: string, errorSummary: string | null) =>
      describeDiscoveryResult({ status, errorSummary, toolCount: 0 }, server)

    expect(
      explain('auth_required', 'server auth mode requires per-user credentials'),
    ).toMatchObject({ title: 'Authentication required', retry: true })
    expect(explain('request_failed', 'fetch failed')).toMatchObject({
      title: 'Discovery failed',
      message:
        "The discovery request didn't complete. Retry, or check the gateway logs if it keeps failing.",
      detail: 'fetch failed',
    })
    // Discovery never runs for a disabled server, so there's nothing to retry.
    expect(explain('disabled', 'external MCP server is disabled')).toMatchObject({
      title: 'Discovery disabled',
      retry: false,
    })
  })

  it('reports a successful refresh with its tool count', () => {
    expect(
      describeDiscoveryResult({ status: 'success', errorSummary: null, toolCount: 2 }, server),
    ).toEqual({
      failed: false,
      title: 'Discovery refreshed',
      message: 'Found 2 tools.',
      detail: null,
      retry: false,
    })
  })

  it.each([
    [800, '800 ms'],
    [1_500, '1.5 s'],
    [90_000, '1 min 30 s'],
    [120_000, '2 min'],
  ])('states a %i ms timeout with a scaled unit', (timeoutMs, timeout) => {
    const { message } = describeDiscoveryResult(
      { status: 'failed', errorSummary: 'MCP upstream timed out', toolCount: 0 },
      { ...server, timeout_ms: timeoutMs },
    )
    expect(message).toContain(`Timed out after ${timeout} waiting`)
  })
})
