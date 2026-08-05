import {
  GitPullRequestIcon,
  HomeIcon,
  McpServerIcon,
  Notification03Icon,
  SaveMoneyDollarIcon,
  SearchIcon,
  RoboticIcon,
  UserIcon,
  UserGroupIcon,
  WaterfallUp02Icon,
} from '@hugeicons/core-free-icons'

import type { AdminPage, AuthSessionView } from '@/types/api'

export interface AdminNavItem {
  page: AdminPage
  label: string
  to: string
  icon: unknown
}

export interface AdminNavSection {
  label: string
  icon: unknown
  items: AdminNavItem[]
}

export const adminNavSections: AdminNavSection[] = [
  {
    label: 'Control Plane',
    icon: SearchIcon,
    items: [
      { page: 'api_keys', label: 'API Keys', to: '/api-keys', icon: SearchIcon },
      { page: 'models', label: 'Models', to: '/models', icon: HomeIcon },
      { page: 'mcp', label: 'MCP', to: '/mcp', icon: McpServerIcon },
      {
        page: 'review_agent',
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
        label: 'Usage Costs',
        to: '/observability/usage-costs',
        icon: SaveMoneyDollarIcon,
      },
      {
        page: 'spend_controls',
        label: 'Spend Controls',
        to: '/spend-controls',
        icon: Notification03Icon,
      },
      {
        page: 'leaderboard',
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
        label: 'Agent Harnesses',
        to: '/observability/agent-harnesses',
        icon: RoboticIcon,
      },
      {
        page: 'request_logs',
        label: 'Request Logs',
        to: '/observability/request-logs',
        icon: SearchIcon,
      },
      {
        page: 'mcp_invocations',
        label: 'MCP Invocations',
        to: '/observability/mcp-invocations',
        icon: McpServerIcon,
      },
    ],
  },
  {
    label: 'Identity',
    icon: UserIcon,
    items: [
      { page: 'teams', label: 'Teams', to: '/identity/teams', icon: UserGroupIcon },
      { page: 'users', label: 'Users', to: '/identity/users', icon: UserIcon },
      {
        page: 'service_accounts',
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
      items: section.items.filter((item) => allowedPages.has(item.page)),
    }))
    .filter((section) => section.items.length > 0)
}

export function getAdminPagePath(page: AdminPage) {
  return adminNavItems().find((item) => item.page === page)?.to
}

export function getAdminPageForPath(path: string) {
  const currentPath = normalizeAdminPath(path.split(/[?#]/, 1)[0])
  return adminNavItems().find((item) => matchesAdminPath(currentPath, item.to))?.page
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
  return adminNavSections.flatMap((section) => section.items).values()
}
