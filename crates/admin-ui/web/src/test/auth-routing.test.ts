import { describe, expect, it } from 'vitest'

import { canAccessAdminPath, defaultSignedInPath, isAdminSession } from '@/routes/-auth-routing'
import type { AuthSessionView } from '@/types/api'

function session(overrides: Partial<AuthSessionView['capabilities']>): AuthSessionView {
  return {
    user: {
      id: 'user-1',
      name: 'Admin',
      email: 'admin@example.com',
      global_role: overrides.platform_admin ? 'platform_admin' : 'member',
    },
    team_id: overrides.platform_admin ? null : 'team-1',
    team_role: overrides.platform_admin ? null : 'admin',
    capabilities: {
      platform_admin: false,
      agent_analysis: false,
      passive_analysis_enabled: true,
      shadow_diagnostics_visible: false,
      calibrated_score_visible: false,
      team_admin_analytics_enabled: false,
      ...overrides,
    },
    must_change_password: false,
  }
}

describe('admin route capabilities', () => {
  it('keeps platform admins signed in when analysis presentation is disabled', () => {
    const platformAdmin = session({ platform_admin: true })

    expect(isAdminSession(platformAdmin)).toBe(true)
    expect(canAccessAdminPath(platformAdmin, '/api-keys')).toBe(true)
    expect(canAccessAdminPath(platformAdmin, '/observability/agent-sessions')).toBe(false)
    expect(defaultSignedInPath(platformAdmin)).toBe('/api-keys')
  })

  it('limits calibrated team admins to agent sessions', () => {
    const teamAdmin = session({
      agent_analysis: true,
      calibrated_score_visible: true,
      team_admin_analytics_enabled: true,
    })

    expect(isAdminSession(teamAdmin)).toBe(true)
    expect(canAccessAdminPath(teamAdmin, '/observability/agent-sessions')).toBe(true)
    expect(canAccessAdminPath(teamAdmin, '/observability/agent-sessions/session-1')).toBe(true)
    expect(canAccessAdminPath(teamAdmin, '/api-keys')).toBe(false)
    expect(defaultSignedInPath(teamAdmin)).toBe('/observability/agent-sessions')
  })

  it('rejects ordinary members from the admin application', () => {
    const member = session({})

    expect(isAdminSession(member)).toBe(false)
    expect(canAccessAdminPath(member, '/observability/agent-sessions')).toBe(false)
  })
})
