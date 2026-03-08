/// <reference types="vite/client" />

import type { ReactNode } from 'react'
import type { QueryClient } from '@tanstack/react-query'
import {
  HeadContent,
  Outlet,
  Scripts,
  createRootRouteWithContext,
  useRouterState,
} from '@tanstack/react-router'
import { Toaster } from 'sonner'

import { AppShell } from '@/components/layout/app-shell'
import globalsCss from '@/styles/globals.css?url'

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()({
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
  const currentPath = pathname.replace(/^\/admin(?=\/|$)/, '') || '/'
  const isPublicRoute =
    currentPath.startsWith('/invite/') || currentPath.startsWith('/account-ready')

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
