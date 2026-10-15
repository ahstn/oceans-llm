import { useState, useTransition } from 'react'
import { useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { disconnectAuthorizedMcpClient } from '@/server/admin-data.functions'
import type { AuthorizedMcpClientView, AuthorizedMcpClientsView } from '@/types/api'

const utcFormatter = new Intl.DateTimeFormat('en-US', {
  dateStyle: 'medium',
  timeStyle: 'short',
  timeZone: 'UTC',
})

/**
 * MCP clients the user allowed through OAuth sign-in. Hidden when the
 * gateway does not offer OAuth sign-in for MCP clients.
 */
export function AuthorizedMcpClientsCard({ clients }: { clients: AuthorizedMcpClientsView }) {
  const router = useRouter()
  const [pendingGrantId, setPendingGrantId] = useState<string | null>(null)
  const [isPending, startTransition] = useTransition()

  if (!clients.enabled) return null

  function disconnect(client: AuthorizedMcpClientView) {
    setPendingGrantId(client.grant_id)
    startTransition(async () => {
      try {
        await disconnectAuthorizedMcpClient({ data: { grantId: client.grant_id } })
        await router.invalidate()
        toast.success(`${clientName(client)} disconnected`)
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Unable to disconnect the client')
      } finally {
        setPendingGrantId(null)
      }
    })
  }

  return (
    <Card data-testid="authorized-mcp-clients">
      <CardHeader>
        <CardTitle>Authorized MCP clients</CardTitle>
        <CardDescription>
          Clients you allowed to use Oceans MCP by signing in. They act as you with your MCP access,
          and stop working when you disconnect them or sign out of the session that approved them.
        </CardDescription>
      </CardHeader>
      <CardContent className="min-w-0">
        {clients.items.length === 0 ? (
          <p className="text-muted-foreground text-sm">No MCP clients are authorized.</p>
        ) : (
          <>
            <ul className="flex flex-col gap-3 md:hidden">
              {clients.items.map((client) => (
                <li
                  key={client.grant_id}
                  className="border-border flex min-w-0 flex-col gap-2 border-b pb-3"
                >
                  <ClientIdentity client={client} />
                  <p className="text-subtle-foreground text-sm">{accessLabel(client)}</p>
                  <p className="text-muted-foreground text-xs">
                    Last token {formatUtc(client.last_token_issued_at)} UTC · Expires{' '}
                    {formatUtc(client.expires_at)} UTC
                  </p>
                  <DisconnectButton
                    client={client}
                    pending={isPending && pendingGrantId === client.grant_id}
                    onDisconnect={disconnect}
                  />
                </li>
              ))}
            </ul>
            <div className="border-border hidden overflow-hidden rounded-md border md:block">
              <Table className="text-left">
                <TableHeader className="bg-surface-muted">
                  <TableRow>
                    <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
                      Client
                    </TableHead>
                    <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
                      Access
                    </TableHead>
                    <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
                      Last token (UTC)
                    </TableHead>
                    <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
                      Expires (UTC)
                    </TableHead>
                    <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
                      Actions
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {clients.items.map((client) => (
                    <TableRow key={client.grant_id}>
                      <TableCell className="px-3 py-3">
                        <ClientIdentity client={client} />
                      </TableCell>
                      <TableCell className="text-subtle-foreground px-3 py-3">
                        {accessLabel(client)}
                      </TableCell>
                      <TableCell className="text-subtle-foreground px-3 py-3">
                        {formatUtc(client.last_token_issued_at)}
                      </TableCell>
                      <TableCell className="text-subtle-foreground px-3 py-3">
                        {formatUtc(client.expires_at)}
                      </TableCell>
                      <TableCell className="px-3 py-3">
                        <DisconnectButton
                          client={client}
                          pending={isPending && pendingGrantId === client.grant_id}
                          onDisconnect={disconnect}
                        />
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          </>
        )}
      </CardContent>
    </Card>
  )
}

function ClientIdentity({ client }: { client: AuthorizedMcpClientView }) {
  return (
    <div className="min-w-0">
      <p className="text-foreground truncate font-semibold">{clientName(client)}</p>
      <p className="text-muted-foreground truncate font-mono text-xs">{client.client_id}</p>
    </div>
  )
}

function DisconnectButton({
  client,
  pending,
  onDisconnect,
}: {
  client: AuthorizedMcpClientView
  pending: boolean
  onDisconnect: (client: AuthorizedMcpClientView) => void
}) {
  return (
    <Button
      variant="outline"
      size="sm"
      disabled={pending}
      aria-label={`Disconnect ${clientName(client)}`}
      onClick={() => onDisconnect(client)}
    >
      {pending ? 'Disconnecting…' : 'Disconnect'}
    </Button>
  )
}

function clientName(client: AuthorizedMcpClientView) {
  return client.client_name?.trim() || 'Unnamed MCP client'
}

function accessLabel(client: AuthorizedMcpClientView) {
  return client.resource_kind === 'aggregate'
    ? 'All MCP servers you have access to'
    : `MCP server ${client.server_key ?? ''}`.trim()
}

function formatUtc(timestamp: string) {
  return utcFormatter.format(new Date(timestamp))
}
