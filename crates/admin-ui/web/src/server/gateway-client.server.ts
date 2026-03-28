import createClient from 'openapi-fetch'
import { getRequest, setResponseHeader } from '@tanstack/react-start/server'

import type { GatewayPaths } from '@/types/live-api'

const DEFAULT_DEV_UI_PORT = '3001'
const DEFAULT_GATEWAY_PORT = '8080'

function trimOrigin(value: string) {
  return value.replace(/\/$/, '')
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

  const requestUrl = new URL(request.url)
  if (requestUrl.port === DEFAULT_DEV_UI_PORT) {
    requestUrl.port = DEFAULT_GATEWAY_PORT
    return trimOrigin(requestUrl.origin)
  }

  const forwardedProto = request.headers.get('x-forwarded-proto') ?? 'http'
  const forwardedHost = request.headers.get('x-forwarded-host') ?? request.headers.get('host')
  if (forwardedHost) {
    return trimOrigin(`${forwardedProto}://${forwardedHost}`)
  }

  return trimOrigin(requestUrl.origin)
}

function resolveGatewayOrigin() {
  const request = getRequest()
  return resolveGatewayOriginFromRequest(request, process.env.ADMIN_GATEWAY_ORIGIN)
}

export function forwardRequestHeadersFromRequest(request: Request, initHeaders?: HeadersInit) {
  const headers = new Headers(initHeaders)
  const requestUrl = new URL(request.url)
  const requestProto =
    request.headers.get('x-forwarded-proto') ?? requestUrl.protocol.replace(/:$/, '')
  const requestHost =
    request.headers.get('x-forwarded-host') ?? request.headers.get('host') ?? requestUrl.host
  const requestOrigin = request.headers.get('x-forwarded-origin') ?? requestUrl.origin

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

export function createGatewayApiClient() {
  return createClient<GatewayPaths>({
    baseUrl: resolveGatewayOrigin(),
    fetch: gatewayFetch,
  })
}

export function unwrapGatewayResponse<TData>(
  result: { data?: TData; error?: unknown; response: Response },
): TData {
  if (result.data !== undefined) {
    return result.data
  }

  throw new Error(readGatewayErrorBody(result.error, result.response.status))
}
