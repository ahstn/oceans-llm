import { createFileRoute } from '@tanstack/react-router'

import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { requireAdminSession } from '@/routes/-admin-guard'
import { getModels } from '@/server/admin-data.functions'

export const Route = createFileRoute('/models')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: () => getModels(),
  component: ModelsPage,
})

function ModelsPage() {
  const { data } = Route.useLoaderData()

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      {data.map((model) => (
        <Card key={model.id}>
          <CardHeader>
            <CardTitle className="flex items-center justify-between">
              <span>{model.id}</span>
              <Badge variant={model.status === 'healthy' ? 'success' : 'warning'}>
                {model.status}
              </Badge>
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-2 text-sm text-neutral-300">
            <p>
              <span className="text-neutral-500">Provider:</span> {model.provider}
            </p>
            <p>
              <span className="text-neutral-500">Upstream:</span> {model.upstreamModel}
            </p>
            <div className="flex flex-wrap gap-2 pt-1">
              {model.tags.map((tag) => (
                <Badge key={tag}>{tag}</Badge>
              ))}
            </div>
          </CardContent>
        </Card>
      ))}
    </div>
  )
}
