import type { AdminPage, AuthSessionView } from '@/types/api'

export const sharedAdminPages: AdminPage[] = [
  'api_keys',
  'models',
  'usage_costs',
  'leaderboard',
  'agent_harnesses',
  'request_logs',
  'mcp_invocations',
  'teams',
  'users',
  'service_accounts',
]

export const allAdminPages: AdminPage[] = [
  'api_keys',
  'models',
  'mcp',
  'review_agent',
  'usage_costs',
  'spend_controls',
  'leaderboard',
  'agent_harnesses',
  'request_logs',
  'mcp_invocations',
  'teams',
  'users',
  'service_accounts',
]

export function platformAdminSession(pages = allAdminPages): AuthSessionView {
  return {
    must_change_password: false,
    permissions: {
      group: 'platform_admins',
      pages,
      default_page: pages.includes('api_keys') ? 'api_keys' : pages[0],
    },
    user: {
      id: 'admin_1',
      name: 'Admin User',
      email: 'admin@example.com',
      global_role: 'platform_admin',
    },
  }
}

export function regularUserSession(pages = sharedAdminPages): AuthSessionView {
  return {
    must_change_password: false,
    permissions: {
      group: 'users',
      pages,
      default_page: pages.includes('usage_costs') ? 'usage_costs' : pages[0],
    },
    user: {
      id: 'user_1',
      name: 'Regular User',
      email: 'user@example.com',
      global_role: 'user',
    },
  }
}
