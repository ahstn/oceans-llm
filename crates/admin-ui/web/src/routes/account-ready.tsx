import { createFileRoute } from '@tanstack/react-router'

import { AuthLayout } from '@/components/layout/auth-layout'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'

export const Route = createFileRoute('/account-ready')({
  component: AccountReadyPage,
})

export function AccountReadyPage() {
  const search = Route.useSearch() as { mode?: string; email?: string }

  return (
    <AuthLayout
      title="Account ready"
      description={
        search.email ? `${search.email} has completed onboarding.` : 'Your account is ready.'
      }
      cardClassName="max-w-2xl"
    >
      <Alert>
        <AlertTitle>
          {search.mode === 'oidc' ? 'SSO onboarding complete' : 'Onboarding complete'}
        </AlertTitle>
        <AlertDescription>
          You can close this page and return to the gateway control-plane workflow.
        </AlertDescription>
      </Alert>
      <div className="flex justify-end">
        <Button asChild>
          <a href="/admin">Open control plane</a>
        </Button>
      </div>
    </AuthLayout>
  )
}
