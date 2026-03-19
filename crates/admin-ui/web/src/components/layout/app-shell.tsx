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
  { label: 'Spend Controls', to: '/spend-controls', icon: Notification03Icon },
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
    <div className="text-foreground min-h-screen px-3 py-3 sm:px-6 sm:py-6">
      <div className="border-border bg-card mx-auto flex h-[calc(100vh-24px)] max-w-[1500px] overflow-hidden rounded-[1.3rem] border sm:h-[calc(100vh-48px)]">
        <aside className="border-border bg-muted hidden w-[292px] shrink-0 border-r p-4 sm:flex sm:flex-col">
          <div className="flex items-center gap-3 px-2 py-1">
            <span className="bg-primary text-primary-foreground inline-flex size-9 items-center justify-center rounded-md text-[11px] font-bold tracking-[0.08em] uppercase">
              OC
            </span>
            <div className="flex min-w-0 flex-col gap-0.5">
              <p className="text-foreground text-sm font-semibold">Oceans Gateway</p>
              <p className="text-muted-foreground/80 text-xs">Control Plane</p>
            </div>
          </div>

          <nav className="mt-6 flex flex-col gap-1.5">
            {topItems.map((item) => (
              <NavLink key={item.to} item={item} currentPath={currentPath} />
            ))}
          </nav>

          <NavGroup label="Observability" items={observabilityItems} currentPath={currentPath} />
          <NavGroup label="Identity Management" items={identityItems} currentPath={currentPath} />

          <div className="border-border mt-auto border-t pt-4">
            <p className="text-muted-foreground/80 text-xs font-semibold tracking-[0.08em] uppercase">
              Local-only preview mode
            </p>
            <p className="text-muted-foreground mt-2 text-sm">
              Identity and spend controls are gateway-backed. API keys and model inventory still use
              local preview data in this environment.
            </p>
          </div>
        </aside>

        <div className="flex min-w-0 flex-1 flex-col">
          <header className="border-border flex h-16 items-center justify-between border-b px-4 sm:px-6">
            <div className="flex flex-col gap-1">
              <p className="text-muted-foreground/80 text-xs font-semibold tracking-[0.14em] uppercase">
                Gateway Admin
              </p>
              <p className="text-foreground text-lg font-semibold">Operations Console</p>
            </div>
            <div className="border-border bg-muted hidden items-center gap-2 rounded-full border px-3 py-1.5 sm:flex">
              <AppIcon icon={SearchIcon} size={16} stroke={1.2} className="text-primary" />
              <span className="text-muted-foreground text-xs font-medium">
                Server-first, same-origin
              </span>
            </div>
          </header>

          <nav className="border-border flex gap-2 overflow-x-auto border-b px-4 py-3 sm:hidden">
            {[...topItems, ...observabilityItems, ...identityItems].map((item) => (
              <Link
                key={item.to}
                to={item.to}
                className={cn(
                  'border-border bg-muted text-muted-foreground shrink-0 rounded-full border px-3 py-1.5 text-xs font-medium',
                  currentPath === item.to && 'border-ring bg-primary/20 text-foreground',
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
      <p className="text-muted-foreground/80 px-2 text-[11px] font-semibold tracking-[0.14em] uppercase">
        {label}
      </p>
      <div className="mt-2 flex flex-col gap-1.5">
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
        'group text-muted-foreground flex items-center gap-3 rounded-lg border border-transparent px-3 py-2.5 text-sm font-medium transition-colors',
        nested && 'ml-2',
        active
          ? 'border-ring bg-primary/20 text-foreground'
          : 'hover:border-border hover:bg-card hover:text-foreground',
      )}
    >
      <AppIcon
        icon={item.icon}
        size={16}
        stroke={1.2}
        className={cn(active ? 'text-primary' : 'text-muted-foreground/80')}
      />
      <span>{item.label}</span>
    </Link>
  )
}
