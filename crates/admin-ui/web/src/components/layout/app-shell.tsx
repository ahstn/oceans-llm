import { type ReactNode, useMemo } from 'react'
import { HomeIcon, Notification03Icon, SearchIcon, UserIcon } from '@hugeicons/core-free-icons'
import { Link, useRouterState } from '@tanstack/react-router'

import { AppIcon } from '@/components/icons/app-icon'
import { cn } from '@/lib/utils'

interface AppShellProps {
  children: ReactNode
}

interface NavItem {
  label: string
  to: string
  icon: unknown
}

const topItems: NavItem[] = [
  { label: 'API Keys', to: '/api-keys', icon: SearchIcon },
  { label: 'Models', to: '/models', icon: HomeIcon },
]

const observabilityItems: NavItem[] = [
  { label: 'Usage Costs', to: '/observability/usage-costs', icon: Notification03Icon },
  { label: 'Request Logs', to: '/observability/request-logs', icon: SearchIcon },
]

const identityItems: NavItem[] = [
  { label: 'Teams', to: '/identity/teams', icon: UserIcon },
  { label: 'Users', to: '/identity/users', icon: UserIcon },
]

export function AppShell({ children }: AppShellProps) {
  const pathname = useRouterState({ select: (state) => state.location.pathname })
  const currentPath = useMemo(() => pathname.replace(/^\/admin/, '') || '/', [pathname])

  return (
    <div className="min-h-screen bg-[radial-gradient(circle_at_top,_#1f3f73_0%,_#1c1c1c_32%)] p-3 text-[14px] text-neutral-200 sm:p-6">
      <div className="mx-auto flex h-[calc(100vh-24px)] max-w-[1400px] overflow-hidden rounded-xl border border-neutral-800 bg-[#131313]/95 shadow-2xl sm:h-[calc(100vh-48px)]">
        <aside className="hidden w-[270px] shrink-0 border-r border-neutral-800 bg-[#151515] p-3 sm:flex sm:flex-col">
          <div className="flex items-center gap-2 rounded-md border border-neutral-800 bg-neutral-950/50 px-3 py-2">
            <span className="inline-flex h-6 w-6 items-center justify-center rounded-md bg-[--color-primary] text-[10px] font-semibold text-black">
              OC
            </span>
            <div>
              <p className="text-sm font-medium text-neutral-100">Oceans Gateway</p>
              <p className="text-xs text-neutral-400">Control Plane</p>
            </div>
          </div>

          <nav className="mt-4 space-y-1">
            {topItems.map((item) => (
              <NavLink key={item.to} item={item} currentPath={currentPath} />
            ))}
          </nav>

          <NavGroup label="Observability" items={observabilityItems} currentPath={currentPath} />
          <NavGroup label="Identity Management" items={identityItems} currentPath={currentPath} />

          <div className="mt-auto rounded-md border border-neutral-800 bg-neutral-950/40 p-3">
            <p className="text-xs text-neutral-400">Local-only preview mode</p>
            <p className="mt-1 text-xs text-neutral-500">
              Server functions currently return mock control-plane data.
            </p>
          </div>
        </aside>

        <div className="flex min-w-0 flex-1 flex-col">
          <header className="flex h-14 items-center justify-between border-b border-neutral-800 px-4 sm:px-6">
            <div>
              <p className="text-xs tracking-[0.08em] text-neutral-500 uppercase">Gateway Admin</p>
              <p className="text-sm font-medium text-neutral-100">Minimal Stack Console</p>
            </div>
            <div className="hidden items-center gap-2 rounded-md border border-neutral-800 bg-neutral-950/60 px-2 py-1 sm:flex">
              <AppIcon icon={SearchIcon} size={16} stroke={1.2} className="text-neutral-400" />
              <span className="text-xs text-neutral-400">Server-first, same-origin</span>
            </div>
          </header>

          <nav className="flex gap-2 overflow-x-auto border-b border-neutral-800 px-4 py-2 sm:hidden">
            {[...topItems, ...observabilityItems, ...identityItems].map((item) => (
              <Link
                key={item.to}
                to={item.to}
                className={cn(
                  'shrink-0 rounded-md border border-neutral-800 px-2 py-1 text-xs text-neutral-300',
                  currentPath === item.to && 'border-green-500/40 bg-green-500/10 text-green-300',
                )}
              >
                {item.label}
              </Link>
            ))}
          </nav>

          <main className="min-h-0 flex-1 overflow-auto p-4 sm:p-6">{children}</main>
        </div>
      </div>
    </div>
  )
}

function NavGroup({
  label,
  items,
  currentPath,
}: {
  label: string
  items: NavItem[]
  currentPath: string
}) {
  return (
    <div className="mt-5">
      <p className="px-2 text-[11px] font-medium tracking-[0.08em] text-neutral-500 uppercase">
        {label}
      </p>
      <div className="mt-2 space-y-1">
        {items.map((item) => (
          <NavLink key={item.to} item={item} currentPath={currentPath} nested />
        ))}
      </div>
    </div>
  )
}

function NavLink({
  item,
  currentPath,
  nested = false,
}: {
  item: NavItem
  currentPath: string
  nested?: boolean
}) {
  const active = currentPath === item.to

  return (
    <Link
      to={item.to}
      className={cn(
        'group flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-neutral-300 transition-colors',
        nested && 'ml-2',
        active
          ? 'bg-neutral-900 text-neutral-100 shadow-[inset_0_0_0_1px_rgba(255,255,255,0.04)]'
          : 'hover:bg-neutral-900/70 hover:text-neutral-100',
      )}
    >
      <AppIcon
        icon={item.icon}
        size={16}
        stroke={1.2}
        className={cn(active ? 'text-[--color-primary]' : 'text-neutral-500')}
      />
      <span>{item.label}</span>
    </Link>
  )
}
