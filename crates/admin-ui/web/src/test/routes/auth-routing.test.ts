import { describe, expect, it } from 'vitest'

import {
  canAccessSignedInPath,
  defaultSignedInPath,
  postLoginAdminHref,
} from '@/routes/-auth-routing'
import { platformAdminSession, regularUserSession } from '@/test/auth-session'
import type { AuthSessionView } from '@/types/api'

const adminSession = platformAdminSession()
const userSession = regularUserSession()

describe('signed-in route selection', () => {
  it('uses role-specific default routes', () => {
    expect(defaultSignedInPath(adminSession)).toBe('/api-keys')
    expect(defaultSignedInPath(userSession)).toBe('/observability/usage-costs')
  })

  it('allows regular users to return to self-service routes', () => {
    expect(canAccessSignedInPath(userSession, '/api-keys')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/models?page=2')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/identity/teams')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/identity/users?user_id=user_2')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/identity/service-accounts')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/observability/leaderboard')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/observability/agent-harnesses')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/observability/request-logs?status=failed')).toBe(
      true,
    )
    expect(postLoginAdminHref(userSession, '/observability/mcp-invocations')).toBe(
      '/admin/observability/mcp-invocations',
    )
  })

  it('replaces a redirect to a page that is absent from the resolved set', () => {
    const modelsOnlySession = regularUserSession(['models'])

    expect(canAccessSignedInPath(modelsOnlySession, '/identity/service-accounts')).toBe(false)
    expect(postLoginAdminHref(modelsOnlySession, '/identity/service-accounts')).toBe(
      '/admin/models',
    )
  })

  it('uses the no-access route when the resolved set is empty', () => {
    const noAccessSession: AuthSessionView = {
      ...regularUserSession([]),
      permissions: {
        group: 'users',
        pages: [],
        default_page: null,
      },
    }

    expect(defaultSignedInPath(noAccessSession)).toBe('/no-access')
    expect(canAccessSignedInPath(noAccessSession, '/no-access')).toBe(true)
  })
})
