import { createFileRoute } from '@tanstack/react-router'

import { AuthLayout } from '@/components/layout/auth-layout'
import { Button } from '@/components/ui/button'

export const Route = createFileRoute('/account-ready')({
  component: AccountReadyPage,
})

export function AccountReadyPage() {
  const search = Route.useSearch() as { email?: string }

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
