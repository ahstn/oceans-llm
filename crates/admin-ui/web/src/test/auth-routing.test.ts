import { describe, expect, it } from 'vitest'

import { canAccessSignedInPath, defaultSignedInPath } from '@/routes/-auth-routing'
import type { AuthSessionView } from '@/types/api'

function session(overrides: Partial<AuthSessionView['capabilities']>): AuthSessionView {
  const pages: AuthSessionView['permissions']['pages'] = [
    'api_keys',
    ...(overrides.agent_analysis ? (['agent_sessions'] as const) : []),
  ]
  return {
    user: {
      id: 'user-1',
      name: 'Admin',
      email: 'admin@example.com',
      global_role: overrides.platform_admin ? 'platform_admin' : 'user',
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
    permissions: {
      group: overrides.platform_admin ? 'platform_admins' : 'team_admins',
      pages,
      actions: [],
      default_page: overrides.agent_analysis ? 'agent_sessions' : 'api_keys',
    },
  }
}

describe('admin route capabilities', () => {
  it('keeps platform admins signed in when analysis presentation is disabled', () => {
    const platformAdmin = session({ platform_admin: true })

    expect(canAccessSignedInPath(platformAdmin, '/api-keys')).toBe(true)
    expect(canAccessSignedInPath(platformAdmin, '/observability/agent-sessions')).toBe(false)
    expect(defaultSignedInPath(platformAdmin)).toBe('/api-keys')
  })

  it('limits calibrated team admins to agent sessions', () => {
    const teamAdmin = session({
      agent_analysis: true,
      calibrated_score_visible: true,
      team_admin_analytics_enabled: true,
    })

    expect(canAccessSignedInPath(teamAdmin, '/observability/agent-sessions')).toBe(true)
    expect(canAccessSignedInPath(teamAdmin, '/observability/agent-sessions/session-1')).toBe(true)
    expect(canAccessSignedInPath(teamAdmin, '/api-keys')).toBe(true)
    expect(defaultSignedInPath(teamAdmin)).toBe('/observability/agent-sessions')
  })

  it('keeps ordinary users out of agent sessions', () => {
    const member = session({})

    expect(canAccessSignedInPath(member, '/api-keys')).toBe(true)
    expect(canAccessSignedInPath(member, '/observability/agent-sessions')).toBe(false)
  })
})
