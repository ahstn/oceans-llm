import {
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
      { label: 'API Keys', to: '/api-keys', icon: SearchIcon },
      { label: 'Models', to: '/models', icon: HomeIcon },
      { label: 'MCP', to: '/mcp', icon: McpServerIcon },
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
      },
      {
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
        label: 'Agent Harnesses',
        to: '/observability/agent-harnesses',
        icon: RoboticIcon,
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
    ],
  },
]

export function normalizeAdminPath(pathname: string) {
  return pathname.replace(/^\/admin(?=\/|$)/, '') || '/'
}

export function getActiveNavSection(currentPath: string) {
  return adminNavSections.find((section) =>
    section.items.some((item) => matchesAdminPath(currentPath, item.to)),
  )
}

export function getActiveNavItem(currentPath: string) {
  return adminNavSections
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
