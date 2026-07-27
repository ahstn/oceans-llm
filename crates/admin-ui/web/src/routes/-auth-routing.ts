import type { AuthSessionView } from '@/types/api'

export const DEFAULT_SIGNED_IN_PATH = '/api-keys'
export const DEFAULT_TEAM_ADMIN_PATH = '/observability/agent-sessions'

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
  if (
    currentPath === DEFAULT_TEAM_ADMIN_PATH ||
    currentPath.startsWith(`${DEFAULT_TEAM_ADMIN_PATH}/`)
  ) {
    return session.capabilities.agent_analysis
  }
  return session.capabilities.platform_admin
}

export function defaultSignedInPath(session: AuthSessionView) {
  return session.capabilities.platform_admin ? DEFAULT_SIGNED_IN_PATH : DEFAULT_TEAM_ADMIN_PATH
}

export function signedInAdminHref(redirect?: string) {
  return `/admin${redirect ?? DEFAULT_SIGNED_IN_PATH}`
}

export function postLoginAdminHref(session: AuthSessionView, redirect?: string) {
  if (session.must_change_password) {
    return redirect
      ? `/admin/change-password?redirect=${encodeURIComponent(redirect)}`
      : '/admin/change-password'
  }

  return signedInAdminHref(redirect ?? defaultSignedInPath(session))
}
