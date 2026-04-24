import { createIsomorphicFn } from '@tanstack/react-start'
import { redirect } from '@tanstack/react-router'

import { getAuthSession } from '@/server/admin-data.functions'
import { buildRedirectTarget, isPlatformAdminSession } from '@/routes/-auth-routing'

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
