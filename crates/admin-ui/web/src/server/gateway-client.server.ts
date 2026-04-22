import createClient from 'openapi-fetch'
import { getRequest, setResponseHeader } from '@tanstack/react-start/server'

import type { GatewayPaths } from '@/types/live-api'

const DEFAULT_DEV_UI_PORT = '3001'
const DEFAULT_GATEWAY_PORT = '8080'

function trimOrigin(value: string) {
  return value.replace(/\/$/, '')
}

function isLoopbackHostname(hostname: string) {
  return hostname === 'localhost' || hostname === '::1' || hostname.startsWith('127.')
}

function parseRequestTarget(request: Request) {
  const requestUrl = new URL(request.url)
  const protocol =
    request.headers.get('x-forwarded-proto') ?? requestUrl.protocol.replace(/:$/, '')
  const forwardedHost = request.headers.get('x-forwarded-host') ?? request.headers.get('host')

  if (forwardedHost) {
    try {
      return new URL(`${protocol}://${forwardedHost}`)
    } catch {
      // Fall back to the request URL when the forwarded host is malformed.
    }
  }

  return requestUrl
}

export function resolveGatewayOriginFromRequest(request: Request, explicitOrigin?: string) {
  const explicit = explicitOrigin?.trim()
  if (explicit) {
    return trimOrigin(explicit)
  }

  const forwardedOrigin = request.headers.get('x-forwarded-origin')
  if (forwardedOrigin) {
    return trimOrigin(forwardedOrigin)
  }

  const requestTarget = parseRequestTarget(request)
  if (requestTarget.port === DEFAULT_DEV_UI_PORT) {
    const gatewayOrigin = new URL(requestTarget.origin)
    if (isLoopbackHostname(gatewayOrigin.hostname)) {
      gatewayOrigin.hostname = '127.0.0.1'
    }
    gatewayOrigin.port = DEFAULT_GATEWAY_PORT
    return trimOrigin(gatewayOrigin.origin)
  }

  return trimOrigin(requestTarget.origin)
}

function resolveGatewayOrigin() {
  const request = getRequest()
  return resolveGatewayOriginFromRequest(request, process.env.ADMIN_GATEWAY_ORIGIN)
}

export function forwardRequestHeadersFromRequest(request: Request, initHeaders?: HeadersInit) {
  const headers = new Headers(initHeaders)
  const requestTarget = parseRequestTarget(request)
  const requestProto =
    request.headers.get('x-forwarded-proto') ?? requestTarget.protocol.replace(/:$/, '')
  const requestHost = request.headers.get('x-forwarded-host') ?? requestTarget.host
  const requestOrigin = request.headers.get('x-forwarded-origin') ?? requestTarget.origin

  const cookie = request.headers.get('cookie')
  if (cookie && !headers.has('cookie')) {
    headers.set('cookie', cookie)
  }

  if (!headers.has('x-forwarded-origin')) {
    headers.set('x-forwarded-origin', requestOrigin)
  }

  if (!headers.has('x-forwarded-proto')) {
    headers.set('x-forwarded-proto', requestProto)
  }

  if (!headers.has('x-forwarded-host')) {
    headers.set('x-forwarded-host', requestHost)
  }

  return headers
}

function forwardRequestHeaders(initHeaders?: HeadersInit) {
  return forwardRequestHeadersFromRequest(getRequest(), initHeaders)
}

function mergeRequestHeaders(input: RequestInfo | URL, initHeaders?: HeadersInit) {
  const headers = new Headers(input instanceof Request ? input.headers : undefined)

  if (initHeaders) {
    const overrides = new Headers(initHeaders)
    overrides.forEach((value, key) => {
      headers.set(key, value)
    })
  }

  return headers
}

function readGatewayErrorBody(error: unknown, status: number) {
  if (
    error &&
    typeof error === 'object' &&
    'error' in error &&
    error.error &&
    typeof error.error === 'object' &&
    'message' in error.error &&
    typeof error.error.message === 'string'
  ) {
    return error.error.message
  }

  return `Gateway request failed with ${status}`
}

async function gatewayFetch(input: RequestInfo | URL, init?: RequestInit) {
  const request = new Request(input, {
    ...init,
    headers: forwardRequestHeaders(mergeRequestHeaders(input, init?.headers)),
  })
  const response = await fetch(request)

  const setCookie = response.headers.get('set-cookie')
  if (setCookie) {
    setResponseHeader('set-cookie', setCookie)
  }

  return response
}

export async function fetchGatewayJson<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await gatewayFetch(new URL(path, resolveGatewayOrigin()), init)
  if (!response.ok) {
    let errorBody: unknown
    try {
      errorBody = await response.json()
    } catch {
      errorBody = undefined
    }
    throw new Error(readGatewayErrorBody(errorBody, response.status))
  }

  return (await response.json()) as T
}

export function createGatewayApiClient() {
  return createClient<GatewayPaths>({
    baseUrl: resolveGatewayOrigin(),
    fetch: gatewayFetch,
  })
}

export function unwrapGatewayResponse<TData>(result: {
  data?: TData
  error?: unknown
  response: Response
}): TData {
  if (result.data !== undefined) {
    return result.data
  }

  throw new Error(readGatewayErrorBody(result.error, result.response.status))
}
