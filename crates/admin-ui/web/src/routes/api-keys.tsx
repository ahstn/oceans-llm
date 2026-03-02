import { createFileRoute } from '@tanstack/react-router'

import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { getApiKeys } from '@/server/admin-data.functions'

export const Route = createFileRoute('/api-keys')({
  loader: () => getApiKeys(),
  component: ApiKeysPage,
})

function ApiKeysPage() {
  const { data } = Route.useLoaderData()

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle>API Keys</CardTitle>
          <p className="text-xs text-neutral-400">
            Server-function-backed mock data using v1 envelope contracts.
          </p>
        </CardHeader>
        <CardContent>
          <div className="overflow-hidden rounded-md border border-neutral-800">
            <table className="w-full text-left text-sm">
              <thead className="bg-neutral-900/70 text-neutral-400">
                <tr>
                  <th className="px-3 py-2 font-medium">Name</th>
                  <th className="px-3 py-2 font-medium">Prefix</th>
                  <th className="px-3 py-2 font-medium">Created</th>
                  <th className="px-3 py-2 font-medium">Status</th>
                </tr>
              </thead>
              <tbody>
                {data.items.map((item) => (
                  <tr key={item.id} className="border-t border-neutral-800">
                    <td className="px-3 py-2">{item.name}</td>
                    <td className="px-3 py-2 font-mono text-xs text-neutral-400">{item.prefix}</td>
                    <td className="px-3 py-2 text-neutral-400">{item.createdAt}</td>
                    <td className="px-3 py-2">
                      <Badge variant={item.status === 'active' ? 'success' : 'warning'}>
                        {item.status}
                      </Badge>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
