import { useState } from 'react'
import { useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { AdminTableHeader } from '@/components/shared/admin-table-header'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { disconnectAuthorizedMcpClient } from '@/server/admin-data.functions'
import type { AuthorizedMcpClientView, AuthorizedMcpClientsView } from '@/types/api'

const COLUMNS = ['Client', 'Access', 'Last token', 'Expires', 'Actions'] as const

const utcFormatter = new Intl.DateTimeFormat('en-US', {
  dateStyle: 'medium',
  timeStyle: 'short',
  timeZone: 'UTC',
})

/**
 * MCP clients the user allowed through OAuth sign-in. Hidden when the
 * gateway does not offer OAuth sign-in for MCP clients and none are left.
 */
export function AuthorizedMcpClientsCard({ clients }: { clients: AuthorizedMcpClientsView }) {
  const router = useRouter()
  // One entry per disconnect in flight, so overlapping clicks on different
  // rows each keep their own button disabled until their request settles.
  const [pendingGrantIds, setPendingGrantIds] = useState<ReadonlySet<string>>(() => new Set())

  // While OAuth sign-in is off, still show clients that would work again if
  // it were turned back on, so they can be disconnected.
  if (!clients.enabled && clients.items.length === 0) return null

  async function disconnect(client: AuthorizedMcpClientView) {
    const grantId = client.grant_id
    setPendingGrantIds((current) => new Set(current).add(grantId))
    try {
      await disconnectAuthorizedMcpClient({ data: { grantId } })
      await router.invalidate()
      toast.success(`${clientName(client)} disconnected`)
    } catch (error) {
      toast.error(error instanceof Error ? error.message : 'Unable to disconnect the client')
    } finally {
      setPendingGrantIds((current) => {
        const next = new Set(current)
        next.delete(grantId)
        return next
      })
    }
  }

  return (
    <Card data-testid="authorized-mcp-clients">
      <CardHeader>
        <CardTitle>Authorized MCP clients</CardTitle>
        <CardDescription>
          Clients you allowed to use Oceans MCP by signing in. They act as you with your MCP access,
          and stop working when you disconnect them or sign out of the session that approved them.
          Times are UTC.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex min-w-0 flex-col gap-3">
        {clients.enabled ? null : <DormantClientsNote />}
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
                    Last token {formatUtc(client.last_token_issued_at)} · Expires{' '}
                    {formatUtc(client.expires_at)}
                  </p>
                  <DisconnectButton
                    client={client}
                    pending={pendingGrantIds.has(client.grant_id)}
                    onDisconnect={(target) => void disconnect(target)}
                  />
                </li>
              ))}
            </ul>
            <div
              className="border-border hidden max-w-full overflow-x-auto rounded-md border md:block"
              data-testid="authorized-mcp-clients-table"
            >
              <Table className="text-left">
                <AdminTableHeader columns={COLUMNS} />
                <TableBody>
                  {clients.items.map((client) => (
                    <TableRow key={client.grant_id}>
                      <TableCell className="max-w-56 px-3 py-3">
                        <ClientIdentity client={client} />
                      </TableCell>
                      <TableCell className="text-subtle-foreground max-w-56 truncate px-3 py-3">
                        {accessLabel(client)}
                      </TableCell>
                      <TableCell className="text-subtle-foreground px-3 py-3">
                        {formatUtc(client.last_token_issued_at)}
                      </TableCell>
                      <TableCell className="text-subtle-foreground px-3 py-3">
                        {formatUtc(client.expires_at)}
                      </TableCell>
                      <TableCell className="w-px px-3 py-3 whitespace-nowrap">
                        <DisconnectButton
                          client={client}
                          pending={pendingGrantIds.has(client.grant_id)}
                          onDisconnect={(target) => void disconnect(target)}
                        />
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
            {clients.has_more ? (
              <p
                className="text-muted-foreground text-sm"
                data-testid="authorized-mcp-clients-more"
              >
                Showing your {clients.items.length} most recent clients. Disconnect some to see
                older ones.
              </p>
            ) : null}
          </>
        )}
      </CardContent>
    </Card>
  )
}

/** OAuth sign-in is off, but these clients would work again if it came back. */
function DormantClientsNote() {
  return (
    <p className="text-muted-foreground text-sm" data-testid="authorized-mcp-clients-dormant">
      OAuth sign-in for MCP clients is turned off, so these clients cannot connect now. They will
      work again if it is turned back on. Disconnect any you no longer use.
    </p>
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
    ? 'All your MCP servers'
    : `MCP server ${client.server_key ?? ''}`.trim()
}

function formatUtc(timestamp: string) {
  return utcFormatter.format(new Date(timestamp))
}
