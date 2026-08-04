import { useEffect, useState, useTransition } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  connectMcpOauthServer,
  disconnectMcpOauthServer,
  getMcpOauthConnections,
} from '@/server/admin-data.functions'
import type { McpOauthConnectionView } from '@/types/api'

type ConnectionsSearch = {
  oauth?: string
  oauth_error?: string
}

const utcTimestampFormatter = new Intl.DateTimeFormat('en-US', {
  dateStyle: 'medium',
  timeStyle: 'short',
  timeZone: 'UTC',
})

export const Route = createFileRoute('/account/connections')({
  validateSearch: (search: Record<string, unknown>): ConnectionsSearch => ({
    oauth: typeof search.oauth === 'string' ? search.oauth : undefined,
    oauth_error: typeof search.oauth_error === 'string' ? search.oauth_error : undefined,
  }),
  loader: () => getMcpOauthConnections(),
  component: ConnectionsPage,
})

export function ConnectionsPage() {
  const connections = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()
  const [pendingServerId, setPendingServerId] = useState<string | null>(null)
  const [isPending, startTransition] = useTransition()

  useEffect(() => {
    if (search.oauth === 'connected') {
      toast.success('Google Workspace connection saved')
    } else if (search.oauth_error) {
      toast.error(connectionErrorMessage(search.oauth_error))
    }
  }, [search.oauth, search.oauth_error])

  function connect(connection: McpOauthConnectionView) {
    setPendingServerId(connection.server_id)
    startTransition(async () => {
      try {
        const response = await connectMcpOauthServer({
          data: { serverId: connection.server_id },
        })
        window.location.assign(response.authorization_url)
      } catch (error) {
        setPendingServerId(null)
        toast.error(error instanceof Error ? error.message : 'Unable to start Google consent')
      }
    })
  }

  function disconnect(connection: McpOauthConnectionView) {
    setPendingServerId(connection.server_id)
    startTransition(async () => {
      try {
        await disconnectMcpOauthServer({ data: { serverId: connection.server_id } })
        await router.invalidate()
        toast.success(`${connection.display_name} disconnected`)
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Unable to disconnect account')
      } finally {
        setPendingServerId(null)
      }
    })
  }

  return (
    <div className="flex min-w-0 flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Workspace connections</CardTitle>
          <CardDescription>
            Connect your Google account for MCP calls made with your user-owned Oceans API keys.
            Oceans stores and refreshes the Google tokens. Client harnesses only receive your Oceans
            endpoint and API key.
          </CardDescription>
        </CardHeader>
      </Card>

      {connections.length === 0 ? (
        <Alert>
          <AlertTitle>No OAuth servers are available</AlertTitle>
          <AlertDescription>
            A platform admin must register the recommended Google Drive or Google Docs MCP server
            and configure its OAuth provider.
          </AlertDescription>
        </Alert>
      ) : (
        <div className="grid gap-4 lg:grid-cols-2">
          {connections.map((connection) => {
            const pending = isPending && pendingServerId === connection.server_id
            const connected = connection.status === 'connected'
            const canConnect = !connection.availability_error
            return (
              <Card key={connection.server_id}>
                <CardHeader>
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <CardTitle>{connection.display_name}</CardTitle>
                      <CardDescription>{connection.server_key}</CardDescription>
                    </div>
                    <Badge variant={connected ? 'default' : 'secondary'}>
                      {formatStatus(connection.status)}
                    </Badge>
                  </div>
                </CardHeader>
                <CardContent className="flex flex-col gap-4">
                  <div className="space-y-2">
                    <p className="text-muted-foreground text-sm">Requested scopes</p>
                    <ul className="space-y-1 text-xs">
                      {connection.required_scopes.map((scope) => (
                        <li key={scope} className="font-mono break-all">
                          {scope}
                        </li>
                      ))}
                    </ul>
                  </div>
                  {connection.expires_at ? (
                    <p className="text-muted-foreground text-xs">
                      Access token expires {formatUtcTimestamp(connection.expires_at)} UTC; Oceans
                      refreshes it before use.
                    </p>
                  ) : null}
                  {connection.availability_error ? (
                    <p className="text-destructive text-xs">
                      OAuth is not configured for this server. Ask a platform admin to complete the
                      gateway OAuth configuration.
                    </p>
                  ) : null}
                  <div className="flex justify-end gap-2">
                    {connected ? (
                      <Button
                        variant="destructive"
                        disabled={pending}
                        onClick={() => disconnect(connection)}
                      >
                        {pending ? 'Disconnecting…' : 'Disconnect'}
                      </Button>
                    ) : (
                      <Button disabled={pending || !canConnect} onClick={() => connect(connection)}>
                        {connectionActionLabel(connection.status, pending)}
                      </Button>
                    )}
                  </div>
                </CardContent>
              </Card>
            )
          })}
        </div>
      )}
    </div>
  )
}

function formatStatus(status: McpOauthConnectionView['status']) {
  return status.charAt(0).toUpperCase() + status.slice(1)
}

function formatUtcTimestamp(timestamp: string) {
  return utcTimestampFormatter.format(new Date(timestamp))
}

function connectionActionLabel(status: McpOauthConnectionView['status'], pending: boolean) {
  if (pending) return 'Opening Google…'
  return status === 'expired' ? 'Reconnect' : 'Connect'
}

function connectionErrorMessage(code: string) {
  switch (code) {
    case 'access_denied':
      return 'Google access was denied.'
    case 'state_expired':
      return 'The connection request expired. Start it again.'
    case 'state_invalid':
      return 'The connection request could not be verified. Start it again.'
    default:
      return 'Google did not complete the connection.'
  }
}
