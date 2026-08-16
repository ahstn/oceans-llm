import { useTransition, type ReactNode } from 'react'
import { Link, useRouterState } from '@tanstack/react-router'
import { toast } from 'sonner'

import { AppSidebar } from '@/components/app-sidebar'
import {
  getActiveNavItem,
  getActiveNavSection,
  getAdminNavSections,
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
import { logoutAdminSession } from '@/server/admin-data.functions'
import type { AuthSessionView } from '@/types/api'

interface AppShellProps {
  children: ReactNode
  session: AuthSessionView
  oceansVersion: string | null
}

export function AppShell({ children, session, oceansVersion }: AppShellProps) {
  const pathname = useRouterState({ select: (state) => state.location.pathname })
  const [isSigningOut, startSignOut] = useTransition()
  const currentPath = normalizeAdminPath(pathname)
  const navSections = getAdminNavSections(session.permissions.pages)
  const activeSection = getActiveNavSection(currentPath, navSections)
  const activeItem = getActiveNavItem(currentPath, navSections)

  function handleSignOut() {
    startSignOut(async () => {
      try {
        await logoutAdminSession()
        window.location.replace('/admin/login')
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Unable to sign out')
      }
    })
  }

  return (
    <SidebarProvider>
      <AppSidebar
        currentPath={currentPath}
        session={session}
        oceansVersion={oceansVersion}
        signOutPending={isSigningOut}
        onSignOut={handleSignOut}
      />

      <SidebarInset>
        <header className="border-border/70 bg-background/80 sticky top-0 z-20 flex h-16 shrink-0 items-center gap-3 border-b px-4 backdrop-blur-xl sm:px-6">
          <SidebarTrigger className="-ml-1" />
          <Separator orientation="vertical" className="self-stretch" />

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
        </header>

        <main className="min-h-0 min-w-0 flex-1 overflow-auto">
          <div className="mx-auto flex min-h-full w-full max-w-[1600px] min-w-0 flex-col gap-6 p-4 sm:p-6">
            {children}
          </div>
        </main>
      </SidebarInset>
    </SidebarProvider>
  )
}
