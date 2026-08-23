import {
  GitPullRequestIcon,
  HomeIcon,
  McpServerIcon,
  Notification03Icon,
  SaveMoneyDollarIcon,
  SearchIcon,
  RoboticIcon,
  TaskDaily01Icon,
  UserIcon,
  UserGroupIcon,
  WaterfallUp02Icon,
} from '@hugeicons/core-free-icons'

import type { AdminPage, AuthSessionView } from '@/types/api'

export type AdminRouteId = AdminPage | 'batches'

export interface AdminNavItem {
  page?: AdminRouteId
  requiredPage?: AdminPage
  label: string
  to: string
  icon: unknown
}

export interface AdminNavSection {
  label: string
  icon: unknown
  items: AdminNavItem[]
}

export const connectionsNavItem: AdminNavItem = {
  label: 'Connections',
  to: '/account/connections',
  icon: McpServerIcon,
}

export const adminNavSections: AdminNavSection[] = [
  {
    label: 'Control Plane',
    icon: SearchIcon,
    items: [
      {
        page: 'api_keys',
        requiredPage: 'api_keys',
        label: 'API Keys',
        to: '/api-keys',
        icon: SearchIcon,
      },
      { page: 'models', requiredPage: 'models', label: 'Models', to: '/models', icon: HomeIcon },
      connectionsNavItem,
      { page: 'mcp', requiredPage: 'mcp', label: 'MCP', to: '/mcp', icon: McpServerIcon },
      {
        page: 'review_agent',
        requiredPage: 'review_agent',
        label: 'Review Agent',
        to: '/review-agent',
        icon: GitPullRequestIcon,
      },
    ],
  },
  {
    label: 'Budget & Spending',
    icon: SaveMoneyDollarIcon,
    items: [
      {
        page: 'usage_costs',
        requiredPage: 'usage_costs',
        label: 'Usage Costs',
        to: '/observability/usage-costs',
        icon: SaveMoneyDollarIcon,
      },
      {
        page: 'spend_controls',
        requiredPage: 'spend_controls',
        label: 'Spend Controls',
        to: '/spend-controls',
        icon: Notification03Icon,
      },
      {
        page: 'leaderboard',
        requiredPage: 'leaderboard',
        label: 'Leaderboard',
        to: '/observability/leaderboard',
        icon: WaterfallUp02Icon,
      },
    ],
  },
  {
    label: 'Observability',
    icon: Notification03Icon,
    items: [
      {
        page: 'agent_harnesses',
        requiredPage: 'agent_harnesses',
        label: 'Agent Harnesses',
        to: '/observability/agent-harnesses',
        icon: RoboticIcon,
      },
      {
        page: 'agent_sessions',
        requiredPage: 'agent_sessions',
        label: 'Agent Sessions',
        to: '/observability/agent-sessions',
        icon: RoboticIcon,
      },
      {
        page: 'request_logs',
        requiredPage: 'request_logs',
        label: 'Request Logs',
        to: '/observability/request-logs',
        icon: SearchIcon,
      },
      {
        page: 'batches',
        requiredPage: 'request_logs',
        label: 'Batch Requests',
        to: '/batches',
        icon: TaskDaily01Icon,
      },
      {
        page: 'mcp_invocations',
        requiredPage: 'mcp_invocations',
        label: 'MCP Invocations',
        to: '/observability/mcp-invocations',
        icon: McpServerIcon,
      },
      {
        requiredPage: 'mcp_invocations',
        label: 'Guardrails',
        to: '/observability/guardrails',
        icon: Notification03Icon,
      },
    ],
  },
  {
    label: 'Identity',
    icon: UserIcon,
    items: [
      {
        page: 'teams',
        requiredPage: 'teams',
        label: 'Teams',
        to: '/identity/teams',
        icon: UserGroupIcon,
      },
      {
        page: 'users',
        requiredPage: 'users',
        label: 'Users',
        to: '/identity/users',
        icon: UserIcon,
      },
      {
        page: 'service_accounts',
        requiredPage: 'service_accounts',
        label: 'Service Accounts',
        to: '/identity/service-accounts',
        icon: RoboticIcon,
      },
    ],
  },
]

export function normalizeAdminPath(pathname: string) {
  return pathname.replace(/^\/admin(?=\/|$)/, '') || '/'
}

export function getAdminNavSections(pages: AdminPage[]) {
  const allowedPages = new Set(pages)
  return adminNavSections
    .map((section) => ({
      ...section,
      items: section.items.filter(
        (item) => !item.requiredPage || allowedPages.has(item.requiredPage),
      ),
    }))
    .filter((section) => section.items.length > 0)
}

export function getAdminPagePath(page: AdminPage) {
  return adminNavItems().find((item) => item.page === page)?.to
}

export function getAdminPageForPath(path: string) {
  const currentPath = normalizeAdminPath(path.split(/[?#]/, 1)[0])
  return adminNavItems().find((item) => matchesAdminPath(currentPath, item.to))?.requiredPage
}

export function canAccessPage(session: AuthSessionView, page: AdminPage) {
  return session.permissions.pages.includes(page)
}

export function getActiveNavSection(
  currentPath: string,
  sections: AdminNavSection[] = adminNavSections,
) {
  return sections.find((section) =>
    section.items.some((item) => matchesAdminPath(currentPath, item.to)),
  )
}

export function getActiveNavItem(
  currentPath: string,
  sections: AdminNavSection[] = adminNavSections,
) {
  return sections
    .flatMap((section) => section.items)
    .find((item) => matchesAdminPath(currentPath, item.to))
}

export function matchesAdminPath(currentPath: string, to: string) {
  const current = stripTrailingSlash(currentPath)
  const target = stripTrailingSlash(to)
  // Exact match, or a deeper subpath (e.g. /mcp stays active on /mcp/servers).
  return current === target || current.startsWith(`${target}/`)
}

function stripTrailingSlash(path: string) {
  return path.length > 1 ? path.replace(/\/+$/, '') : path
}

function adminNavItems() {
  return adminNavSections.flatMap((section) => section.items)
}
