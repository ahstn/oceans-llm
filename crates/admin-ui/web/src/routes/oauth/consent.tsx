import { useState, useTransition } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { decideOauthAuthorization, getOauthAuthorization } from '@/server/admin-data.functions'
import type { OauthAuthorizationRequestView } from '@/types/api'

type ConsentSearch = {
  /** Set by the gateway's `/oauth/authorize` redirect. */
  request_id?: string
}

type ConsentLoaderData =
  | { status: 'ready'; request: OauthAuthorizationRequestView }
  | { status: 'unavailable' }
  | { status: 'error' }

export const Route = createFileRoute('/oauth/consent')({
  validateSearch: (search: Record<string, unknown>): ConsentSearch => ({
    request_id: typeof search.request_id === 'string' ? search.request_id : undefined,
  }),
  loaderDeps: ({ search }) => ({ requestId: search.request_id }),
  loader: async ({ deps }): Promise<ConsentLoaderData> => {
    if (!deps.requestId) return { status: 'unavailable' }
    try {
      const request = await getOauthAuthorization({ data: { requestId: deps.requestId } })
      // Expired, already decided, or unknown requests all need a fresh sign-in.
      return request ? { status: 'ready', request } : { status: 'unavailable' }
    } catch {
      // The request may still be valid; let the user retry.
      return { status: 'error' }
    }
  },
  component: OauthConsentPage,
})

export function OauthConsentPage() {
  const data = Route.useLoaderData()
  const { session } = Route.useRouteContext()

  if (data.status === 'error') {
    return (
      <Alert variant="destructive" data-testid="oauth-consent-error">
        <AlertTitle>This sign-in request could not be loaded</AlertTitle>
        <AlertDescription>
          <p>Oceans could not reach the gateway. Your request may still be valid.</p>
          <Button variant="outline" size="sm" onClick={() => window.location.reload()}>
            Try again
          </Button>
        </AlertDescription>
      </Alert>
    )
  }

  if (data.status !== 'ready') {
    return (
      <Alert variant="destructive" data-testid="oauth-consent-unavailable">
        <AlertTitle>This sign-in request is no longer valid</AlertTitle>
        <AlertDescription>
          It expired or was already answered. Return to your MCP client and start sign-in again.
        </AlertDescription>
      </Alert>
    )
  }

  return <ConsentCard request={data.request} userEmail={session?.user.email} />
}

function ConsentCard({
  request,
  userEmail,
}: {
  request: OauthAuthorizationRequestView
  userEmail: string | undefined
}) {
  const [decision, setDecision] = useState<'approve' | 'deny' | null>(null)
  const [isPending, startTransition] = useTransition()
  const clientName = request.client_name?.trim() || 'An MCP client'

  function decide(approve: boolean) {
    setDecision(approve ? 'approve' : 'deny')
    startTransition(async () => {
      try {
        const response = await decideOauthAuthorization({
          data: { requestId: request.request_id, approve },
        })
        window.location.assign(response.redirect_to)
      } catch (error) {
        setDecision(null)
        toast.error(error instanceof Error ? error.message : 'Unable to answer the request')
      }
    })
  }

  return (
    <div className="mx-auto flex w-full max-w-xl min-w-0 flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Allow {clientName} to use Oceans MCP?</CardTitle>
          <CardDescription>
            {userEmail ? `Signed in as ${userEmail}. ` : null}
            The client gets the MCP tools you already have access to. It cannot see your API keys or
            call models.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <dl className="grid grid-cols-1 gap-3 text-sm sm:grid-cols-[10rem_1fr]">
            <dt className="text-muted-foreground">Client</dt>
            <dd className="min-w-0 break-words">
              {clientName}
              <span className="text-muted-foreground block text-xs">
                Name provided by the client. Client ID{' '}
                <span className="font-mono break-all">{request.client_id}</span>
              </span>
            </dd>
            <dt className="text-muted-foreground">Returns to</dt>
            <dd className="flex min-w-0 flex-wrap items-center gap-2" data-testid="redirect-host">
              <span className="font-mono break-all">{request.redirect_host}</span>
              {request.redirect_is_loopback ? <Badge variant="warning">This device</Badge> : null}
            </dd>
            <dt className="text-muted-foreground">Access</dt>
            <dd className="min-w-0 break-words">
              {request.resource_kind === 'aggregate'
                ? 'All MCP servers you have access to'
                : (request.server_display_name ?? request.server_key)}
            </dd>
            <dt className="text-muted-foreground">Permissions</dt>
            <dd className="min-w-0">
              <ul className="flex flex-col gap-1">
                {request.scopes.map((scope) => (
                  <li key={scope.scope}>
                    {scope.description}
                    <span className="text-muted-foreground block font-mono text-xs">
                      {scope.scope}
                    </span>
                  </li>
                ))}
              </ul>
            </dd>
          </dl>

          {request.redirect_is_loopback ? (
            <Alert data-testid="loopback-warning">
              <AlertTitle>Only continue if you started this sign-in</AlertTitle>
              <AlertDescription>
                This client runs on your device, so Oceans cannot verify who published it.
              </AlertDescription>
            </Alert>
          ) : null}

          <p className="text-muted-foreground text-xs">
            Access lasts until you sign out of Oceans or the client is disconnected.
          </p>

          <div className="flex justify-end gap-2">
            <Button variant="outline" disabled={isPending} onClick={() => decide(false)}>
              {isPending && decision === 'deny' ? 'Denying…' : 'Deny'}
            </Button>
            <Button disabled={isPending} onClick={() => decide(true)}>
              {isPending && decision === 'approve' ? 'Allowing…' : 'Allow'}
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
