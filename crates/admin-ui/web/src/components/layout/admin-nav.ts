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

export interface AdminNavItem {
  label: string
  to: string
  icon: unknown
  adminOnly?: boolean
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
      { label: 'API Keys', to: '/api-keys', icon: SearchIcon },
      { label: 'Models', to: '/models', icon: HomeIcon },
      connectionsNavItem,
      { label: 'MCP', to: '/mcp', icon: McpServerIcon, adminOnly: true },
      {
        label: 'Review Agent',
        to: '/review-agent',
        icon: GitPullRequestIcon,
        adminOnly: true,
      },
    ],
  },
  {
    label: 'Budget & Spending',
    icon: SaveMoneyDollarIcon,
    items: [
      {
        label: 'Usage Costs',
        to: '/observability/usage-costs',
        icon: SaveMoneyDollarIcon,
      },
      {
        label: 'Spend Controls',
        to: '/spend-controls',
        icon: Notification03Icon,
        adminOnly: true,
      },
      {
        label: 'Leaderboard',
        to: '/observability/leaderboard',
        icon: WaterfallUp02Icon,
        adminOnly: true,
      },
    ],
  },
  {
    label: 'Observability',
    icon: Notification03Icon,
    items: [
      {
        label: 'Agent Harnesses',
        to: '/observability/agent-harnesses',
        icon: RoboticIcon,
        adminOnly: true,
      },
      {
        label: 'Request Logs',
        to: '/observability/request-logs',
        icon: SearchIcon,
      },
      {
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
      { label: 'Teams', to: '/identity/teams', icon: UserGroupIcon },
      { label: 'Users', to: '/identity/users', icon: UserIcon },
      {
        label: 'Service Accounts',
        to: '/identity/service-accounts',
        icon: RoboticIcon,
        adminOnly: true,
      },
    ],
  },
]

export function normalizeAdminPath(pathname: string) {
  return pathname.replace(/^\/admin(?=\/|$)/, '') || '/'
}

export function getAdminNavSections(globalRole: string) {
  if (globalRole === 'platform_admin') {
    return adminNavSections
  }

  return adminNavSections
    .map((section) => ({
      ...section,
      items: section.items.filter((item) => !item.adminOnly),
    }))
    .filter((section) => section.items.length > 0)
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
