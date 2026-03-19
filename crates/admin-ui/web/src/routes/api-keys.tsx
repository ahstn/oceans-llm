import { createFileRoute } from '@tanstack/react-router'
import { SearchIcon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty'
import { requireAdminSession } from '@/routes/-admin-guard'
import { getApiKeys } from '@/server/admin-data.functions'

export const Route = createFileRoute('/api-keys')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: () => getApiKeys(),
  component: ApiKeysPage,
})

function ApiKeysPage() {
  const { data } = Route.useLoaderData()

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-4">
          <div className="flex flex-col gap-1">
            <CardTitle>API Keys</CardTitle>
            <CardDescription>
              Review gateway key identifiers, issuance time, and status before distributing
              credentials.
            </CardDescription>
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {data.items.length === 0 ? (
            <Empty>
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <AppIcon icon={SearchIcon} size={22} stroke={1.5} />
                </EmptyMedia>
                <EmptyTitle>No API keys yet</EmptyTitle>
                <EmptyDescription>
                  Seed or create a gateway key before distributing credentials to downstream
                  clients.
                </EmptyDescription>
              </EmptyHeader>
              <EmptyContent />
            </Empty>
          ) : (
            <>
              <div className="grid gap-3 md:hidden">
                {data.items.map((item) => (
                  <div key={item.id} className="border-border bg-muted rounded-lg border p-4">
                    <div className="flex items-start justify-between gap-3">
                      <div className="flex flex-col gap-1">
                        <p className="text-foreground font-semibold">{item.name}</p>
                        <p className="text-muted-foreground/80 font-mono text-xs">{item.prefix}</p>
                      </div>
                      <Badge variant={item.status === 'active' ? 'success' : 'warning'}>
                        {item.status}
                      </Badge>
                    </div>
                    <p className="text-muted-foreground mt-3 text-sm">Created {item.createdAt}</p>
                  </div>
                ))}
              </div>

              <div className="border-border hidden overflow-hidden rounded-md border md:block">
                <table className="w-full text-left text-sm">
                  <thead className="bg-muted text-muted-foreground/80">
                    <tr>
                      <th className="px-3 py-2 font-semibold">Name</th>
                      <th className="px-3 py-2 font-semibold">Prefix</th>
                      <th className="px-3 py-2 font-semibold">Created</th>
                      <th className="px-3 py-2 font-semibold">Status</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.items.map((item) => (
                      <tr key={item.id} className="border-border border-t">
                        <td className="text-foreground px-3 py-3">{item.name}</td>
                        <td className="text-muted-foreground/80 px-3 py-3 font-mono text-xs">
                          {item.prefix}
                        </td>
                        <td className="text-muted-foreground px-3 py-3">{item.createdAt}</td>
                        <td className="px-3 py-3">
                          <Badge variant={item.status === 'active' ? 'success' : 'warning'}>
                            {item.status}
                          </Badge>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
