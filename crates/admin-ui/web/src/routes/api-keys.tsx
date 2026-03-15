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
                  <div
                    key={item.id}
                    className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4"
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="flex flex-col gap-1">
                        <p className="font-semibold text-[var(--color-text)]">{item.name}</p>
                        <p className="font-mono text-xs text-[var(--color-text-soft)]">
                          {item.prefix}
                        </p>
                      </div>
                      <Badge variant={item.status === 'active' ? 'success' : 'warning'}>
                        {item.status}
                      </Badge>
                    </div>
                    <p className="mt-3 text-sm text-[var(--color-text-muted)]">
                      Created {item.createdAt}
                    </p>
                  </div>
                ))}
              </div>

              <div className="hidden overflow-hidden rounded-md border border-[color:var(--color-border)] md:block">
                <table className="w-full text-left text-sm">
                  <thead className="bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]">
                    <tr>
                      <th className="px-3 py-2 font-semibold">Name</th>
                      <th className="px-3 py-2 font-semibold">Prefix</th>
                      <th className="px-3 py-2 font-semibold">Created</th>
                      <th className="px-3 py-2 font-semibold">Status</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.items.map((item) => (
                      <tr key={item.id} className="border-t border-[color:var(--color-border)]">
                        <td className="px-3 py-3 text-[var(--color-text)]">{item.name}</td>
                        <td className="px-3 py-3 font-mono text-xs text-[var(--color-text-soft)]">
                          {item.prefix}
                        </td>
                        <td className="px-3 py-3 text-[var(--color-text-muted)]">
                          {item.createdAt}
                        </td>
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
