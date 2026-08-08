import { describe, expect, it } from 'vitest'

import {
  canAccessSignedInPath,
  defaultSignedInPath,
  postLoginAdminHref,
} from '@/routes/-auth-routing'
import type { AuthSessionView } from '@/types/api'

const adminSession: AuthSessionView = {
  capabilities: {
    platform_admin: true,
    agent_analysis: false,
    passive_analysis_enabled: true,
    shadow_diagnostics_visible: false,
    calibrated_score_visible: false,
    team_admin_analytics_enabled: false,
  },
  team_id: null,
  team_role: null,
  must_change_password: false,
  user: {
    id: 'admin_1',
    name: 'Admin User',
    email: 'admin@example.com',
    global_role: 'platform_admin',
  },
}

const userSession: AuthSessionView = {
  capabilities: {
    platform_admin: false,
    agent_analysis: false,
    passive_analysis_enabled: true,
    shadow_diagnostics_visible: false,
    calibrated_score_visible: false,
    team_admin_analytics_enabled: false,
  },
  team_id: null,
  team_role: null,
  must_change_password: false,
  user: {
    id: 'user_1',
    name: 'Regular User',
    email: 'user@example.com',
    global_role: 'user',
  },
}

describe('signed-in route selection', () => {
  it('uses role-specific default routes', () => {
    expect(defaultSignedInPath(adminSession)).toBe('/api-keys')
    expect(defaultSignedInPath(userSession)).toBe('/observability/usage-costs')
  })

  it('allows regular users to return to self-service routes', () => {
    expect(canAccessSignedInPath(userSession, '/api-keys')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/models?page=2')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/account/connections')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/identity/teams')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/identity/users?user_id=user_2')).toBe(true)
    expect(canAccessSignedInPath(userSession, '/observability/request-logs?status=failed')).toBe(
      true,
    )
    expect(postLoginAdminHref(userSession, '/observability/mcp-invocations')).toBe(
      '/admin/observability/mcp-invocations',
    )
  })

  it('replaces a regular user redirect to an admin-only route', () => {
    expect(canAccessSignedInPath(userSession, '/identity/service-accounts')).toBe(false)
    expect(postLoginAdminHref(userSession, '/identity/service-accounts')).toBe(
      '/admin/observability/usage-costs',
    )
  })
})
