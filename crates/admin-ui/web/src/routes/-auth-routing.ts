import type { AuthSessionView } from '@/types/api'

export const DEFAULT_SIGNED_IN_PATH = '/api-keys'

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

export function signedInAdminHref(redirect?: string) {
  return `/admin${redirect ?? DEFAULT_SIGNED_IN_PATH}`
}

export function postLoginAdminHref(session: AuthSessionView, redirect?: string) {
  if (session.must_change_password) {
    return redirect
      ? `/admin/change-password?redirect=${encodeURIComponent(redirect)}`
      : '/admin/change-password'
  }

  return signedInAdminHref(redirect)
}
