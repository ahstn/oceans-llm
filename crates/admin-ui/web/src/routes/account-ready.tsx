import { createFileRoute } from '@tanstack/react-router'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'

export const Route = createFileRoute('/account-ready')({
  component: AccountReadyPage,
})

function AccountReadyPage() {
  const search = Route.useSearch() as { mode?: string; email?: string }

  return (
    <div className="min-h-screen bg-[radial-gradient(circle_at_top,_#1f3f73_0%,_#1c1c1c_34%)] p-4 text-neutral-100 sm:p-8">
      <div className="mx-auto flex min-h-[calc(100vh-32px)] max-w-3xl items-center justify-center">
        <Card className="w-full max-w-xl bg-[#131313]/95">
          <CardHeader>
            <CardTitle>Account ready</CardTitle>
            <CardDescription>
              {search.email
                ? `${search.email} has completed onboarding.`
                : 'Your account is ready.'}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Alert>
              <AlertTitle>
                {search.mode === 'oidc' ? 'SSO onboarding complete' : 'Onboarding complete'}
              </AlertTitle>
              <AlertDescription>
                You can close this page and return to the gateway control-plane workflow.
              </AlertDescription>
            </Alert>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
