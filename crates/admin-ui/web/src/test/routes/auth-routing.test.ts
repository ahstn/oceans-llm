import { describe, expect, it } from 'vitest'

import {
  canAccessSignedInPath,
  defaultSignedInPath,
  postLoginAdminHref,
} from '@/routes/-auth-routing'
import { platformAdminSession, regularUserSession } from '@/test/auth-session'

const adminSession = platformAdminSession()
const userSession = regularUserSession()

describe('signed-in route selection', () => {
  it('lands every user with page access on their profile', () => {
    expect(defaultSignedInPath(adminSession)).toBe('/profile')
    expect(defaultSignedInPath(userSession)).toBe('/profile')
    expect(defaultSignedInPath(regularUserSession(['models']))).toBe('/profile')
  })

  it('lets any signed-in user open their profile', () => {
    const modelsOnlySession = regularUserSession(['models'])

    expect(canAccessSignedInPath(modelsOnlySession, '/profile')).toBe(true)
    expect(canAccessSignedInPath(modelsOnlySession, '/profiles')).toBe(false)
  })

  it('allows regular users to return to self-service routes', () => {
    expect(canAccessSignedInPath(userSession, '/api-keys')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/models?page=2')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/account/connections')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/identity/teams')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/identity/users?user_id=user_2')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/identity/service-accounts')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/observability/leaderboard')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/observability/agent-harnesses')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/observability/request-logs?status=failed')).toBe(
      true,
    )
    expect(canAccessSignedInPath(userSession, '/batches?status=completed')).toBe(true)
    expect(postLoginAdminHref(userSession, '/observability/mcp-invocations')).toBe(
      '/admin/observability/mcp-invocations',
    )
  })

  it('reserves guardrail observability for platform admins', () => {
    expect(canAccessSignedInPath(userSession, '/observability/guardrails')).toBe(false)
    expect(canAccessSignedInPath(adminSession, '/observability/guardrails')).toBe(true)
  })

  it('replaces a redirect to a page that is absent from the resolved set', () => {
    const modelsOnlySession = regularUserSession(['models'])

    expect(canAccessSignedInPath(modelsOnlySession, '/identity/service-accounts')).toBe(false)
    expect(canAccessSignedInPath(modelsOnlySession, '/batches')).toBe(false)
    expect(postLoginAdminHref(modelsOnlySession, '/identity/service-accounts')).toBe(
      '/admin/profile',
    )
  })

  it('uses the canonical request-log route while granting access to batches', () => {
    const requestLogsOnlySession = regularUserSession(['request_logs'])

    expect(canAccessSignedInPath(requestLogsOnlySession, '/batches')).toBe(true)
  })

  it('uses the no-access route when the resolved set is empty', () => {
    const noAccessSession = regularUserSession([])

    expect(noAccessSession.permissions.default_page).toBeNull()
    expect(defaultSignedInPath(noAccessSession)).toBe('/no-access')
    expect(canAccessSignedInPath(noAccessSession, '/no-access')).toBe(true)
  })
})
