import type { AdminAction, AdminPage, AuthSessionView } from '@/types/api'

export const userAdminActions: AdminAction[] = [
  'create_api_key',
  'update_api_key',
  'revoke_api_key',
]

export const allAdminActions: AdminAction[] = [...userAdminActions, 'reveal_api_key']

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
      actions: allAdminActions,
      default_page: pages.includes('api_keys') ? 'api_keys' : (pages[0] ?? null),
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
      actions: userAdminActions,
      default_page: pages.includes('usage_costs') ? 'usage_costs' : (pages[0] ?? null),
    },
    user: {
      id: 'user_1',
      name: 'Regular User',
      email: 'user@example.com',
      global_role: 'user',
    },
  }
}
