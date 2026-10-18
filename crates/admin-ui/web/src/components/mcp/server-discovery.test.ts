import { describe, expect, it } from 'vitest'

import { describeDiscoveryResult, type DiscoveryRefreshResult } from './server-discovery'

const server = {
  server_url: 'https://unreachable.invalid/mcp',
  timeout_ms: 30_000,
  auth_mode: 'none',
}

const failed = (fields: Partial<DiscoveryRefreshResult>): DiscoveryRefreshResult => ({
  status: 'failed',
  errorSummary: 'gateway summary',
  toolCount: 0,
  ...fields,
})

describe('describeDiscoveryResult', () => {
  it.each([
    ['transport', null, "Couldn't reach unreachable.invalid. Check the endpoint, then Retry."],
    [
      'timeout',
      null,
      'Timed out after 30 s waiting for unreachable.invalid. Check the endpoint or raise the timeout in Configuration, then Retry.',
    ],
    [
      'http',
      401,
      "Reached unreachable.invalid, but it rejected the gateway's credentials (HTTP 401). Check the auth config, then Retry.",
    ],
    [
      'http',
      404,
      'Reached unreachable.invalid, but found no MCP endpoint at this URL (HTTP 404). Check the endpoint, then Retry.',
    ],
    [
      'http',
      503,
      "Reached unreachable.invalid, but it couldn't handle the request (HTTP 503). Wait a moment, then Retry.",
    ],
    [
      'http',
      418,
      'Reached unreachable.invalid, but it returned an HTTP error (HTTP 418). Check the endpoint and auth config, then Retry.',
    ],
    [
      'json_rpc',
      null,
      "unreachable.invalid answered as an MCP server but refused to list its tools; its error is below. Check the server's access requirements, then Retry.",
    ],
    [
      'invalid_response',
      null,
      "Reached unreachable.invalid, but it didn't respond as an MCP server. Check that the endpoint is a Streamable HTTP MCP URL, then Retry.",
    ],
    [
      'invalid_auth_config',
      null,
      "The gateway couldn't load this server's credentials. Check the auth config, then Retry.",
    ],
    [
      'a_kind_the_ui_has_not_seen',
      null,
      "Discovery didn't complete for unreachable.invalid. Check the endpoint and auth config, then Retry.",
    ],
    [
      null,
      null,
      "Discovery didn't complete for unreachable.invalid. Check the endpoint and auth config, then Retry.",
    ],
  ])(
    'explains error kind %s (HTTP %s) and keeps the summary as detail',
    (errorKind, httpStatus, message) => {
      expect(describeDiscoveryResult(failed({ errorKind, httpStatus }), server)).toEqual({
        failed: true,
        title: 'Discovery failed',
        message,
        detail: 'gateway summary',
        retry: true,
      })
    },
  )

  it('ignores the summary wording when the error kind is known', () => {
    const { message } = describeDiscoveryResult(
      failed({ errorKind: 'transport', errorSummary: 'reworded by a newer gateway' }),
      server,
    )
    expect(message).toBe("Couldn't reach unreachable.invalid. Check the endpoint, then Retry.")
  })

  it('names the next step for failures before the upstream is reached', () => {
    const explain = (status: string, authMode = 'none') =>
      describeDiscoveryResult(
        { status, errorSummary: 'summary', toolCount: 0 },
        {
          ...server,
          auth_mode: authMode,
        },
      )

    expect(explain('auth_required', 'user_passthrough')).toMatchObject({
      title: 'Authentication required',
      message: expect.stringContaining('Switch to a gateway auth mode'),
      retry: true,
    })
    // OAuth keeps its mode; public discovery is the supported fix.
    expect(explain('auth_required', 'oauth_obo').message).toContain(
      'set discovery_auth to none in the auth config',
    )
    expect(explain('auth_required', 'oauth_obo').message).not.toContain('Switch to a gateway')
    expect(explain('request_failed')).toMatchObject({
      title: 'Discovery failed',
      message:
        "The discovery request didn't complete. Retry, or check the gateway logs if it keeps failing.",
    })
    // Discovery never runs for a disabled server, so there's nothing to retry.
    expect(explain('disabled')).toMatchObject({ title: 'Discovery disabled', retry: false })
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
    [59_999, '1 min'],
    [90_000, '1 min 30 s'],
    [119_999, '2 min'],
    [120_000, '2 min'],
  ])('states a %i ms timeout with a scaled unit', (timeoutMs, timeout) => {
    const { message } = describeDiscoveryResult(failed({ errorKind: 'timeout' }), {
      ...server,
      timeout_ms: timeoutMs,
    })
    expect(message).toContain(`Timed out after ${timeout} waiting`)
  })
})
