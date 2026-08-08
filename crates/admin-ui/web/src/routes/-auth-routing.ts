import type { AuthSessionView } from '@/types/api'

export const DEFAULT_SIGNED_IN_PATH = '/api-keys'
export const DEFAULT_USER_PATH = '/observability/usage-costs'
export const AGENT_SESSIONS_PATH = '/observability/agent-sessions'

const USER_ACCESSIBLE_PATHS = [
  '/api-keys',
  '/models',
  '/account/connections',
  '/identity/teams',
  '/identity/users',
  DEFAULT_USER_PATH,
  '/observability/request-logs',
  '/observability/mcp-invocations',
]

export function normalizeAdminPath(pathname: string) {
  return pathname.replace(/^\/admin(?=\/|$)/, '') || '/'
}

export function isPublicAdminRoute(currentPath: string) {
  return (
    currentPath.startsWith('/invite/') ||
    currentPath.startsWith('/account-ready') ||
    currentPath === '/login' ||
    currentPath === '/change-password'
  )
}

export function buildRedirectTarget(pathname: string, search: Record<string, unknown>) {
  const currentPath = normalizeAdminPath(pathname)
  const query = new URLSearchParams()

  for (const [key, value] of Object.entries(search)) {
    if (typeof value === 'string') {
      query.set(key, value)
    }
  }

  const searchString = query.toString()
  return searchString ? `${currentPath}?${searchString}` : currentPath
}

export function isPlatformAdminSession(session: AuthSessionView | null | undefined) {
  return session?.user.global_role === 'platform_admin'
}

export function isAdminSession(session: AuthSessionView | null | undefined) {
  return (
    session?.capabilities.platform_admin === true || session?.capabilities.agent_analysis === true
  )
}

export function canAccessAdminPath(session: AuthSessionView, currentPath: string) {
  if (currentPath === AGENT_SESSIONS_PATH || currentPath.startsWith(`${AGENT_SESSIONS_PATH}/`)) {
    return session.capabilities.agent_analysis
  }
  return session.capabilities.platform_admin
}

export function defaultSignedInPath(session: AuthSessionView) {
  if (isPlatformAdminSession(session)) return DEFAULT_SIGNED_IN_PATH
  return session.capabilities.agent_analysis ? AGENT_SESSIONS_PATH : DEFAULT_USER_PATH
}

export function canAccessSignedInPath(session: AuthSessionView, path: string) {
  const pathname = path.split(/[?#]/, 1)[0]
  if (pathname === AGENT_SESSIONS_PATH || pathname.startsWith(`${AGENT_SESSIONS_PATH}/`)) {
    return session.capabilities.agent_analysis
  }
  if (isPlatformAdminSession(session)) return true
  return USER_ACCESSIBLE_PATHS.some((allowedPath) => pathname === allowedPath)
}

export function signedInAdminHref(session: AuthSessionView, redirect?: string) {
  const target =
    redirect && canAccessSignedInPath(session, redirect) ? redirect : defaultSignedInPath(session)
  return `/admin${target}`
}

export function postLoginAdminHref(session: AuthSessionView, redirect?: string) {
  if (session.must_change_password) {
    return redirect
      ? `/admin/change-password?redirect=${encodeURIComponent(redirect)}`
      : '/admin/change-password'
  }

  return signedInAdminHref(session, redirect)
}
