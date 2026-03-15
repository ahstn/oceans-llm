import { getRequest, setResponseHeader } from '@tanstack/react-start/server'

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

async function readGatewayError(response: Response) {
  try {
    const body = await response.json()
    if (body?.error?.message) {
      return String(body.error.message)
    }
  } catch {
    // Ignore parse failures and use the HTTP status instead.
  }

  return `Gateway request failed with ${response.status}`
}

export async function fetchGatewayJson<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = forwardRequestHeaders(init?.headers)
  const response = await fetch(`${resolveGatewayOrigin()}${path}`, {
    ...init,
    headers,
  })

  const setCookie = response.headers.get('set-cookie')
  if (setCookie) {
    setResponseHeader('set-cookie', setCookie)
  }

  if (!response.ok) {
    throw new Error(await readGatewayError(response))
  }

  return (await response.json()) as T
}
