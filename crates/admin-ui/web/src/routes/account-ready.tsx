import { useEffect } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'

import { AuthLayout } from '@/components/layout/auth-layout'
import { Button } from '@/components/ui/button'

export const Route = createFileRoute('/account-ready')({
  validateSearch: (search: Record<string, unknown>) => ({
    mode:
      search.mode === 'oauth' || search.mode === 'oidc' || search.mode === 'password'
        ? search.mode
        : undefined,
    email: typeof search.email === 'string' ? search.email : undefined,
  }),
  component: AccountReadyPage,
})

export function AccountReadyPage() {
  const search = Route.useSearch()
  const navigate = useNavigate()
  const isSsoOnboarding = search.mode === 'oauth' || search.mode === 'oidc'

  useEffect(() => {
    if (!isSsoOnboarding) return

    const redirectTimeout = window.setTimeout(() => {
      void navigate({ to: '/' })
    }, 1500)

    return () => window.clearTimeout(redirectTimeout)
  }, [isSsoOnboarding, navigate])

  return (
    <AuthLayout
      title="Account ready"
      description={
        search.email ? `${search.email} has completed onboarding.` : 'Your account is ready.'
      }
      cardClassName="max-w-2xl"
    >
      <p className="text-muted-foreground max-w-lg text-base text-pretty">
        Click below to return to the Gateway UI, if you aren&apos;t redirected.
      </p>
      <Button asChild className="w-full" size="lg">
        <a href="/admin">Open control plane</a>
      </Button>
    </AuthLayout>
  )
}
