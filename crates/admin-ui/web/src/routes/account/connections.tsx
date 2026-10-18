import { useEffect, useRef, useState, useTransition } from 'react'
import { createFileRoute, useNavigate, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  connectMcpOauthServer,
  disconnectMcpOauthServer,
  getAuthorizedMcpClients,
  getMcpOauthConnections,
} from '@/server/admin-data.functions'
import type { McpOauthConnectionView } from '@/types/api'

import { AuthorizedMcpClientsCard } from './-authorized-mcp-clients'

type ConnectionsSearch = {
  /** Server key from an MCP `connect_url`; starts the OAuth flow on load. */
  connect?: string
  oauth?: string
  oauth_error?: string
  /** Server ID the OAuth callback returned from. */
  server?: string
}

const utcTimestampFormatter = new Intl.DateTimeFormat('en-US', {
  dateStyle: 'medium',
  timeStyle: 'short',
  timeZone: 'UTC',
})

export const Route = createFileRoute('/account/connections')({
  validateSearch: (search: Record<string, unknown>): ConnectionsSearch => ({
    connect: typeof search.connect === 'string' ? search.connect : undefined,
    oauth: typeof search.oauth === 'string' ? search.oauth : undefined,
    oauth_error: typeof search.oauth_error === 'string' ? search.oauth_error : undefined,
    server: typeof search.server === 'string' ? search.server : undefined,
  }),
  loader: async () => {
    const [connections, authorizedClients] = await Promise.all([
      getMcpOauthConnections(),
      getAuthorizedMcpClients(),
    ])
    return { connections, authorizedClients }
  },
  component: ConnectionsPage,
})

// Redirect and disconnect transitions belong with their single connection-card workflow.
// oxlint-disable-next-line eslint/max-lines-per-function
export function ConnectionsPage() {
  const { connections, authorizedClients } = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()
  const navigate = useNavigate()
  const [pendingServerId, setPendingServerId] = useState<string | null>(null)
  const [isPending, startTransition] = useTransition()

  useEffect(() => {
    if (search.oauth === 'connected') {
      const connection = connections.find((item) => item.server_id === search.server)
      toast.success(connection ? `${connection.display_name} connected` : 'Connection saved')
    } else if (search.oauth_error) {
      toast.error(connectionErrorMessage(search.oauth_error))
    } else {
      return
    }
    // Drop the callback result so a later loader refresh does not repeat the toast.
    void navigate({
      to: '/account/connections',
      search: (previous: ConnectionsSearch) => ({
        ...previous,
        oauth: undefined,
        oauth_error: undefined,
        server: undefined,
      }),
      replace: true,
    })
  }, [connections, navigate, search.oauth, search.oauth_error, search.server])

  useConnectRequest(search.connect, connections, connect)

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
        toast.error(error instanceof Error ? error.message : 'Unable to start the connection')
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
            Connect your accounts on OAuth-protected MCP servers for calls made with your user-owned
            Oceans API keys. Oceans stores and refreshes the tokens. Client harnesses only receive
            your Oceans endpoint and API key.
          </CardDescription>
        </CardHeader>
      </Card>

      {connections.length === 0 ? (
        <Alert>
          <AlertTitle>No OAuth servers are available</AlertTitle>
          <AlertDescription>
            A platform admin must register an MCP server that uses per-user OAuth, such as the
            recommended Google Drive server or any server that supports OAuth discovery.
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
                      <CardDescription>
                        {connection.server_key} · via {connection.provider_label}
                      </CardDescription>
                    </div>
                    <Badge variant={connected ? 'success' : 'secondary'}>
                      {formatStatus(connection.status)}
                    </Badge>
                  </div>
                </CardHeader>
                <CardContent className="flex flex-col gap-4">
                  <div className="space-y-2">
                    <p className="text-muted-foreground text-sm">Requested scopes</p>
                    {connection.required_scopes.length === 0 ? (
                      <p className="text-xs">Provider default</p>
                    ) : null}
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
                    <p className="text-destructive text-xs">{connection.availability_error}</p>
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

      <AuthorizedMcpClientsCard clients={authorizedClients} />
    </div>
  )
}

/**
 * Starts the OAuth flow for the server named by `?connect=`, the link MCP
 * clients receive with `credential_required` and `credential_expired` errors.
 * The parameter is dropped first so a reload or the OAuth return does not
 * start the flow again.
 */
function useConnectRequest(
  serverKey: string | undefined,
  connections: McpOauthConnectionView[],
  connect: (connection: McpOauthConnectionView) => void,
) {
  const navigate = useNavigate()
  const handledKey = useRef<string | null>(null)

  useEffect(() => {
    if (!serverKey) {
      // The parameter was dropped, so a later link to the same server is a new request.
      handledKey.current = null
      return
    }
    if (handledKey.current === serverKey) return
    handledKey.current = serverKey
    void navigate({
      to: '/account/connections',
      search: (previous: ConnectionsSearch) => ({ ...previous, connect: undefined }),
      replace: true,
    })

    const connection = connections.find((item) => item.server_key === serverKey)
    if (!connection) {
      toast.error(`No workspace connection is available for MCP server ${serverKey}.`)
    } else if (connection.availability_error) {
      toast.error(connection.availability_error)
    } else if (connection.status === 'connected') {
      toast.info(`${connection.display_name} is already connected`)
    } else {
      connect(connection)
    }
  }, [serverKey, connections, connect, navigate])
}

function formatStatus(status: McpOauthConnectionView['status']) {
  return status.charAt(0).toUpperCase() + status.slice(1)
}

function formatUtcTimestamp(timestamp: string) {
  return utcTimestampFormatter.format(new Date(timestamp))
}

function connectionActionLabel(status: McpOauthConnectionView['status'], pending: boolean) {
  if (pending) return 'Opening sign-in…'
  return status === 'expired' ? 'Reconnect' : 'Connect'
}

function connectionErrorMessage(code: string) {
  switch (code) {
    case 'access_denied':
      return 'Access was denied by the provider.'
    case 'state_expired':
      return 'The connection request expired. Start it again.'
    case 'state_invalid':
      return 'The connection request could not be verified. Start it again.'
    default:
      return 'The provider did not complete the connection.'
  }
}
