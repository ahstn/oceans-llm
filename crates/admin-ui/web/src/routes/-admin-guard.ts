import { createIsomorphicFn } from '@tanstack/react-start'
import { redirect } from '@tanstack/react-router'

import { getAuthSession } from '@/server/admin-data.functions'

const loadAuthSession = createIsomorphicFn()
  .server(async () => {
    const { getSession } = await import('@/server/admin-data.server')
    return getSession()
  })
  .client(() => getAuthSession())

function normalizeAdminPath(pathname: string) {
  return pathname.replace(/^\/admin(?=\/|$)/, '') || '/'
}

function buildRedirectTarget(pathname: string, search: Record<string, unknown>) {
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

export async function requireAdminSession(location: {
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
