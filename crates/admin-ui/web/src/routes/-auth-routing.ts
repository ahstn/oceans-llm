import type { AuthSessionView } from '@/types/api'

export const DEFAULT_SIGNED_IN_PATH = '/api-keys'
export const DEFAULT_USER_PATH = '/observability/usage-costs'

const USER_ACCESSIBLE_PATHS = [
  '/api-keys',
  '/models',
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

export function defaultSignedInPath(session: AuthSessionView) {
  return isPlatformAdminSession(session) ? DEFAULT_SIGNED_IN_PATH : DEFAULT_USER_PATH
}

export function canAccessSignedInPath(session: AuthSessionView, path: string) {
  if (isPlatformAdminSession(session)) return true
  const pathname = path.split(/[?#]/, 1)[0]
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
