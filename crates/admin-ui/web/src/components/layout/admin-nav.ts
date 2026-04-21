import { HomeIcon, Notification03Icon, SearchIcon, UserIcon } from '@hugeicons/core-free-icons'

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
      { label: 'Spend Controls', to: '/spend-controls', icon: Notification03Icon },
    ],
  },
  {
    label: 'Observability',
    icon: Notification03Icon,
    items: [
      {
        label: 'Leaderboard',
        to: '/observability/leaderboard',
        icon: Notification03Icon,
      },
      {
        label: 'Usage Costs',
        to: '/observability/usage-costs',
        icon: Notification03Icon,
      },
      {
        label: 'Request Logs',
        to: '/observability/request-logs',
        icon: SearchIcon,
      },
    ],
  },
  {
    label: 'Identity',
    icon: UserIcon,
    items: [
      { label: 'Teams', to: '/identity/teams', icon: UserIcon },
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
  return currentPath === to
}
