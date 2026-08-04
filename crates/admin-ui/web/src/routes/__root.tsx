/// <reference types="vite/client" />

import type { ReactNode } from 'react'
import type { QueryClient } from '@tanstack/react-query'
import { createIsomorphicFn } from '@tanstack/react-start'
import {
  HeadContent,
  Navigate,
  Outlet,
  Scripts,
  createRootRouteWithContext,
  redirect,
  useRouterState,
} from '@tanstack/react-router'
import { Toaster } from 'sonner'

import { AppShell } from '@/components/layout/app-shell'
import { GlobalErrorPage } from '@/components/layout/global-error-page'
import { TooltipProvider } from '@/components/ui/tooltip'
import { getAuthSession, getOceansVersion } from '@/server/admin-data.functions'
import globalsCss from '@/styles/globals.css?url'
import faviconUrl from '@/assets/oceans-logo-rounded-square.png?url'
import {
  buildRedirectTarget,
  defaultPathForSession,
  isPlatformAdminSession,
  isPublicAdminRoute,
  isSelfServicePath,
  normalizeAdminPath,
} from '@/routes/-auth-routing'

const loadAuthSession = createIsomorphicFn()
  .server(async () => {
    const { getSession } = await import('@/server/admin-data.server')
    return getSession()
  })
  .client(() => getAuthSession())

const loadOceansVersion = createIsomorphicFn()
  .server(async () => {
    const { getGatewayVersion } = await import('@/server/admin-data.server')
    return getGatewayVersion()
  })
  .client(() => getOceansVersion())

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  beforeLoad: async ({ location }) => {
    const currentPath = normalizeAdminPath(location.pathname)
    const isPublicRoute = isPublicAdminRoute(currentPath)
    const { data: session } = await loadAuthSession()

    if (isPublicRoute) {
      if (currentPath === '/login' && session) {
        throw redirect({
          to: session.must_change_password ? '/change-password' : defaultPathForSession(session),
        })
      }

      if (currentPath === '/change-password' && !session) {
        throw redirect({
          to: '/login',
          search: { redirect: '/change-password' },
        })
      }

      return { session, oceansVersion: null }
    }

    if (!session || (!isPlatformAdminSession(session) && !isSelfServicePath(currentPath))) {
      throw redirect({
        to: '/login',
        search: {
          redirect: buildRedirectTarget(location.pathname, location.search),
        },
      })
    }

    if (session.must_change_password && currentPath !== '/change-password') {
      throw redirect({ to: '/change-password' })
    }

    const oceansVersion = await loadOceansVersion().catch(() => null)
    return { session, oceansVersion }
  },
  errorComponent: RootErrorComponent,
  head: () => ({
    meta: [
      { charSet: 'utf-8' },
      { name: 'viewport', content: 'width=device-width, initial-scale=1' },
      { title: 'Oceans Gateway Admin' },
      {
        name: 'description',
        content: 'Oceans LLM gateway control plane powered by TanStack Start',
      },
    ],
    links: [
      { rel: 'stylesheet', href: globalsCss },
      { rel: 'icon', type: 'image/png', href: faviconUrl },
      { rel: 'apple-touch-icon', href: faviconUrl },
    ],
  }),
  component: RootComponent,
})

function RootErrorComponent(props: Parameters<typeof GlobalErrorPage>[0]) {
  return (
    <RootDocument>
      <GlobalErrorPage {...props} />
    </RootDocument>
  )
}

function RootComponent() {
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  })
  const currentPath = normalizeAdminPath(pathname)
  const isPublicRoute = isPublicAdminRoute(currentPath)
  const { session, oceansVersion } = Route.useRouteContext()

  if (!isPublicRoute && session?.must_change_password) {
    return (
      <RootDocument>
        <Navigate to="/change-password" />
      </RootDocument>
    )
  }

  return (
    <RootDocument>
      {isPublicRoute ? (
        <Outlet />
      ) : session ? (
        <AppShell session={session} oceansVersion={oceansVersion}>
          <Outlet />
        </AppShell>
      ) : null}
    </RootDocument>
  )
}

function RootDocument({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className="dark">
      <head>
        <HeadContent />
      </head>
      <body>
        <TooltipProvider>{children}</TooltipProvider>
        <Toaster
          position="top-right"
          theme="dark"
          toastOptions={{
            style: {
              background: 'var(--card)',
              border: '1px solid var(--border)',
              color: 'var(--foreground)',
            },
          }}
        />
        <Scripts />
      </body>
    </html>
  )
}
