/// <reference types="vite/client" />

import type { ReactNode } from 'react'
import type { QueryClient } from '@tanstack/react-query'
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
import { getAuthSession } from '@/server/admin-data.functions'
import globalsCss from '@/styles/globals.css?url'

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  beforeLoad: async ({ location }) => {
    const currentPath = normalizeAdminPath(location.pathname)
    const isPublicRoute = isPublicAdminRoute(currentPath)
    const {
      data: session,
    } = await getAuthSession()

    if (isPublicRoute) {
      if (currentPath === '/login' && session) {
        throw redirect({
          to: session.must_change_password ? '/change-password' : '/api-keys',
        })
      }

      if (currentPath === '/change-password' && !session) {
        throw redirect({
          to: '/login',
          search: { redirect: '/change-password' },
        })
      }

      return { session }
    }

    if (!session) {
      throw redirect({
        to: '/login',
        search: { redirect: buildRedirectTarget(location.pathname, location.search) },
      })
    }

    if (session.must_change_password && currentPath !== '/change-password') {
      throw redirect({ to: '/change-password' })
    }

    return { session }
  },
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
    links: [{ rel: 'stylesheet', href: globalsCss }],
  }),
  component: RootComponent,
})

function RootComponent() {
  const pathname = useRouterState({ select: (state) => state.location.pathname })
  const currentPath = normalizeAdminPath(pathname)
  const isPublicRoute = isPublicAdminRoute(currentPath)
  const { session } = Route.useRouteContext()

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
      ) : (
        <AppShell>
          <Outlet />
        </AppShell>
      )}
    </RootDocument>
  )
}

function normalizeAdminPath(pathname: string) {
  return pathname.replace(/^\/admin(?=\/|$)/, '') || '/'
}

function isPublicAdminRoute(currentPath: string) {
  return (
    currentPath.startsWith('/invite/') ||
    currentPath.startsWith('/account-ready') ||
    currentPath === '/login' ||
    currentPath === '/change-password'
  )
}

function buildRedirectTarget(pathname: string, search: Record<string, unknown>) {
  const currentPath = normalizeAdminPath(pathname)
  const query = new URLSearchParams()

  for (const [key, value] of Object.entries(search)) {
    if (typeof value === 'string') {
      query.set(key, value)
    }
  }

  const searchString = query.toString()
  return searchString ? `${currentPath}?${searchString}` : currentPath
}

function RootDocument({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <head>
        <HeadContent />
      </head>
      <body>
        {children}
        <Toaster
          position="top-right"
          theme="dark"
          toastOptions={{
            style: {
              background: '#151515',
              border: '1px solid #2a2a2a',
              color: '#f5f5f5',
            },
          }}
        />
        <Scripts />
      </body>
    </html>
  )
}
