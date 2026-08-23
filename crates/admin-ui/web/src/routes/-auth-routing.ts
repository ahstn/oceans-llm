import type { AdminAction, AuthSessionView } from '@/types/api'
import {
  canAccessPage,
  getAdminPageForPath,
  getAdminPagePath,
  isPlatformAdminOnlyPath,
  normalizeAdminPath,
} from '@/components/layout/admin-nav'

export { normalizeAdminPath }

const PERSONAL_ACCOUNT_PATHS = new Set(['/account/connections'])

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

export function defaultSignedInPath(session: AuthSessionView) {
  const defaultPage = session.permissions.default_page
  return (defaultPage && getAdminPagePath(defaultPage)) || '/no-access'
}

export function isPlatformAdminSession(session: AuthSessionView | null | undefined) {
  return session?.user.global_role === 'platform_admin'
}

export function canPerformAdminAction(
  session: AuthSessionView | null | undefined,
  action: AdminAction,
) {
  return session?.permissions.actions.includes(action) ?? false
}

export function canAccessSignedInPath(session: AuthSessionView, path: string) {
  const pathname = normalizeAdminPath(path.split(/[?#]/, 1)[0])
  if (pathname === '/') return true
  if (PERSONAL_ACCOUNT_PATHS.has(pathname)) return true
  if (pathname === '/no-access') return session.permissions.pages.length === 0
  if (isPlatformAdminOnlyPath(pathname) && !isPlatformAdminSession(session)) return false
  const page = getAdminPageForPath(pathname)
  return page ? canAccessPage(session, page) : false
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
