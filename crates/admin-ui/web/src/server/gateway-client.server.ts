import { getRequest, setResponseHeader } from '@tanstack/react-start/server'

function resolveGatewayOrigin() {
  const explicit = process.env.ADMIN_GATEWAY_ORIGIN?.trim()
  if (explicit) {
    return explicit.replace(/\/$/, '')
  }

  const request = getRequest()
  const forwardedOrigin = request.headers.get('x-forwarded-origin')
  if (forwardedOrigin) {
    return forwardedOrigin.replace(/\/$/, '')
  }

  const forwardedProto = request.headers.get('x-forwarded-proto') ?? 'http'
  const forwardedHost = request.headers.get('x-forwarded-host') ?? request.headers.get('host')
  if (forwardedHost) {
    return `${forwardedProto}://${forwardedHost}`.replace(/\/$/, '')
  }

  return new URL(request.url).origin.replace(/\/$/, '')
}

function forwardRequestHeaders(initHeaders?: HeadersInit) {
  const request = getRequest()
  const headers = new Headers(initHeaders)

  const cookie = request.headers.get('cookie')
  if (cookie && !headers.has('cookie')) {
    headers.set('cookie', cookie)
  }

  const forwardedOrigin = request.headers.get('x-forwarded-origin')
  if (forwardedOrigin && !headers.has('x-forwarded-origin')) {
    headers.set('x-forwarded-origin', forwardedOrigin)
  }

  const forwardedProto = request.headers.get('x-forwarded-proto')
  if (forwardedProto && !headers.has('x-forwarded-proto')) {
    headers.set('x-forwarded-proto', forwardedProto)
  }

  const forwardedHost = request.headers.get('x-forwarded-host')
  if (forwardedHost && !headers.has('x-forwarded-host')) {
    headers.set('x-forwarded-host', forwardedHost)
  }

  return headers
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
