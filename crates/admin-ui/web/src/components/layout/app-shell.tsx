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
    <div className="min-h-screen px-3 py-3 text-[var(--color-text)] sm:px-6 sm:py-6">
      <div className="mx-auto flex h-[calc(100vh-24px)] max-w-[1500px] overflow-hidden rounded-[1.3rem] border border-[color:var(--color-border)] bg-[color:var(--color-surface)] shadow-[var(--shadow-panel)] sm:h-[calc(100vh-48px)]">
        <aside className="hidden w-[292px] shrink-0 border-r border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4 sm:flex sm:flex-col">
          <div className="flex items-center gap-3 rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface)] px-4 py-3">
            <span className="inline-flex size-9 items-center justify-center rounded-md bg-[var(--color-primary)] text-[11px] font-bold tracking-[0.08em] text-[var(--color-primary-foreground)] uppercase">
              OC
            </span>
            <div className="flex min-w-0 flex-col gap-0.5">
              <p className="text-sm font-semibold text-[var(--color-text)]">Oceans Gateway</p>
              <p className="text-xs text-[var(--color-text-soft)]">Control Plane</p>
            </div>
          </div>

          <nav className="mt-6 flex flex-col gap-1.5">
            {topItems.map((item) => (
              <NavLink key={item.to} item={item} currentPath={currentPath} />
            ))}
          </nav>

          <NavGroup label="Observability" items={observabilityItems} currentPath={currentPath} />
          <NavGroup label="Identity Management" items={identityItems} currentPath={currentPath} />

          <div className="mt-auto rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface)] p-4">
            <p className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
              Local-only preview mode
            </p>
            <p className="mt-2 text-sm text-[var(--color-text-muted)]">
              Identity onboarding is wired to the gateway. Other control-plane pages may still use
              local preview data.
            </p>
          </div>
        </aside>

        <div className="flex min-w-0 flex-1 flex-col">
          <header className="flex h-16 items-center justify-between border-b border-[color:var(--color-border)] px-4 sm:px-6">
            <div className="flex flex-col gap-1">
              <p className="text-xs font-semibold tracking-[0.14em] text-[var(--color-text-soft)] uppercase">
                Gateway Admin
              </p>
              <p className="text-lg font-semibold text-[var(--color-text)]">Operations Console</p>
            </div>
            <div className="hidden items-center gap-2 rounded-full border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] px-3 py-1.5 sm:flex">
              <AppIcon
                icon={SearchIcon}
                size={16}
                stroke={1.2}
                className="text-[var(--color-primary)]"
              />
              <span className="text-xs font-medium text-[var(--color-text-muted)]">
                Server-first, same-origin
              </span>
            </div>
          </header>

          <nav className="flex gap-2 overflow-x-auto border-b border-[color:var(--color-border)] px-4 py-3 sm:hidden">
            {[...topItems, ...observabilityItems, ...identityItems].map((item) => (
              <Link
                key={item.to}
                to={item.to}
                className={cn(
                  'shrink-0 rounded-full border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] px-3 py-1.5 text-xs font-medium text-[var(--color-text-muted)]',
                  currentPath === item.to &&
                    'border-[color:var(--color-border-strong)] bg-[var(--color-primary-soft)] text-[var(--color-text)]',
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
      <p className="px-2 text-[11px] font-semibold tracking-[0.14em] text-[var(--color-text-soft)] uppercase">
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
        'group flex items-center gap-3 rounded-lg border border-transparent px-3 py-2.5 text-sm font-medium text-[var(--color-text-muted)] transition-colors',
        nested && 'ml-2',
        active
          ? 'border-[color:var(--color-border-strong)] bg-[var(--color-primary-soft)] text-[var(--color-text)]'
          : 'hover:border-[color:var(--color-border)] hover:bg-[color:var(--color-surface)] hover:text-[var(--color-text)]',
      )}
    >
      <AppIcon
        icon={item.icon}
        size={16}
        stroke={1.2}
        className={cn(active ? 'text-[var(--color-primary)]' : 'text-[var(--color-text-soft)]')}
      />
      <span>{item.label}</span>
    </Link>
  )
}
