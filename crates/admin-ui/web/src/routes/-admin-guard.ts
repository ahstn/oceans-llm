import { redirect } from '@tanstack/react-router'
import { createIsomorphicFn } from '@tanstack/react-start'

import { buildRedirectTarget, isPlatformAdminSession } from '@/routes/-auth-routing'
import { getAuthSession } from '@/server/admin-data.functions'

const loadAuthSession = createIsomorphicFn()
  .server(async () => {
    const { getSession } = await import('@/server/admin-data.server')
    return getSession()
  })
  .client(() => getAuthSession())

export async function requireAdminSession(location: {
  pathname: string
  search: Record<string, unknown>
}) {
  const { data: session } = await loadAuthSession()
  const adminSession = isPlatformAdminSession(session) ? session : null

  if (!adminSession) {
    throw redirect({
      to: '/login',
      search: { redirect: buildRedirectTarget(location.pathname, location.search) },
    })
  }

  if (adminSession.must_change_password) {
    throw redirect({ to: '/change-password' })
  }

  return { session: adminSession }
}

export async function requireAuthenticatedSession(location: {
  pathname: string
  search: Record<string, unknown>
}) {
  const { data: session } = await loadAuthSession()

  if (!session) {
    throw redirect({
      to: '/login',
      search: { redirect: buildRedirectTarget(location.pathname, location.search) },
    })
  }

  if (session.must_change_password) {
    throw redirect({ to: '/change-password' })
  }

  return { session }
}
