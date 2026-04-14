import type { ReactNode } from 'react'
import { Link, useRouterState } from '@tanstack/react-router'

import { AppSidebar } from '@/components/app-sidebar'
import {
  getActiveNavItem,
  getActiveNavSection,
  normalizeAdminPath,
} from '@/components/layout/admin-nav'
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from '@/components/ui/breadcrumb'
import { Separator } from '@/components/ui/separator'
import { SidebarInset, SidebarProvider, SidebarTrigger } from '@/components/ui/sidebar'
import type { AuthSessionView } from '@/types/api'

interface AppShellProps {
  children: ReactNode
  session: AuthSessionView
}

export function AppShell({ children, session }: AppShellProps) {
  const pathname = useRouterState({ select: (state) => state.location.pathname })
  const currentPath = normalizeAdminPath(pathname)
  const activeSection = getActiveNavSection(currentPath)
  const activeItem = getActiveNavItem(currentPath)

  return (
    <SidebarProvider>
      <AppSidebar currentPath={currentPath} session={session} />

      <SidebarInset>
        <header className="border-border/70 bg-background/80 sticky top-0 z-20 flex h-16 shrink-0 items-center gap-3 border-b px-4 backdrop-blur-xl sm:px-6">
          <SidebarTrigger className="-ml-1" />
          <Separator orientation="vertical" className="data-[orientation=vertical]:h-4" />

          <Breadcrumb>
            <BreadcrumbList>
              {activeSection ? (
                <>
                  <BreadcrumbItem className="hidden md:block">
                    <BreadcrumbLink asChild>
                      <Link to={activeSection.items[0].to}>{activeSection.label}</Link>
                    </BreadcrumbLink>
                  </BreadcrumbItem>
                  <BreadcrumbSeparator className="hidden md:block" />
                </>
              ) : null}
              <BreadcrumbItem>
                <BreadcrumbPage>{activeItem?.label ?? 'Operations Console'}</BreadcrumbPage>
              </BreadcrumbItem>
            </BreadcrumbList>
          </Breadcrumb>

          <div className="border-border/70 bg-card/70 text-muted-foreground ml-auto hidden items-center gap-2 rounded-full border px-3 py-1.5 text-xs font-medium sm:flex">
            <span className="bg-primary inline-flex size-2 rounded-full" />
            Server-first · same-origin
          </div>
        </header>

        <main className="min-h-0 flex-1 overflow-auto">
          <div className="mx-auto flex min-h-full w-full max-w-[1600px] flex-col gap-6 p-4 sm:p-6">
            {children}
          </div>
        </main>
      </SidebarInset>
    </SidebarProvider>
  )
}
