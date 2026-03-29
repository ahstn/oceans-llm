import { describe, expect, it } from 'vitest'

import {
  forwardRequestHeadersFromRequest,
  resolveGatewayOriginFromRequest,
} from '@/server/gateway-client.server'

describe('resolveGatewayOriginFromRequest', () => {
  it('targets the gateway port when the UI is accessed directly on the dev server', () => {
    const request = new Request('http://localhost:3001/admin/identity/users', {
      headers: { host: 'localhost:3001' },
    })

    expect(resolveGatewayOriginFromRequest(request)).toBe('http://localhost:8080')
  })

  it('prefers forwarded origin when the UI is accessed through the gateway proxy', () => {
    const request = new Request('http://localhost:3001/admin/identity/users', {
      headers: {
        host: 'localhost:3001',
        'x-forwarded-origin': 'http://localhost:8080',
      },
    })

    expect(resolveGatewayOriginFromRequest(request)).toBe('http://localhost:8080')
  })
})

describe('forwardRequestHeadersFromRequest', () => {
  it('forwards a synthetic origin for direct dev-server requests', () => {
    const request = new Request('http://localhost:3001/admin/identity/users', {
      headers: {
        host: 'localhost:3001',
        cookie: 'ogw_session=test',
      },
    })

    const headers = forwardRequestHeadersFromRequest(request)

    expect(headers.get('cookie')).toBe('ogw_session=test')
    expect(headers.get('x-forwarded-origin')).toBe('http://localhost:3001')
    expect(headers.get('x-forwarded-proto')).toBe('http')
    expect(headers.get('x-forwarded-host')).toBe('localhost:3001')
  })

  it('preserves existing request headers while adding forwarded metadata', () => {
    const request = new Request('http://localhost:3001/admin/login', {
      headers: {
        host: 'localhost:3001',
        cookie: 'ogw_session=test',
      },
    })

    const headers = forwardRequestHeadersFromRequest(request, {
      'content-type': 'application/json',
      accept: 'application/json',
    })

    expect(headers.get('content-type')).toBe('application/json')
    expect(headers.get('accept')).toBe('application/json')
    expect(headers.get('cookie')).toBe('ogw_session=test')
    expect(headers.get('x-forwarded-origin')).toBe('http://localhost:3001')
  })
})
