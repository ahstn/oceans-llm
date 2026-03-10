import { createFileRoute } from '@tanstack/react-router'
import { HomeIcon } from '@hugeicons/core-free-icons'

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
import { getModels } from '@/server/admin-data.functions'

export const Route = createFileRoute('/models')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: () => getModels(),
  component: ModelsPage,
})

function ModelsPage() {
  const { data } = Route.useLoaderData()

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-1">
        <h1 className="text-2xl font-semibold tracking-tight text-[var(--color-text)]">Models</h1>
        <p className="max-w-2xl text-sm text-[var(--color-text-muted)]">
          Review the routed models available to operators, along with upstream targets and current
          health.
        </p>
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        {data.length === 0 ? (
          <Card className="lg:col-span-2">
            <CardContent className="pt-5">
              <Empty>
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <AppIcon icon={HomeIcon} size={22} stroke={1.5} />
                  </EmptyMedia>
                  <EmptyTitle>No models configured</EmptyTitle>
                  <EmptyDescription>
                    Add at least one routed model before sending traffic through the gateway.
                  </EmptyDescription>
                </EmptyHeader>
                <EmptyContent />
              </Empty>
            </CardContent>
          </Card>
        ) : (
          data.map((model) => (
            <Card key={model.id}>
              <CardHeader className="gap-4">
                <div className="flex items-start justify-between gap-3">
                  <div className="flex flex-col gap-1">
                    <CardTitle>{model.id}</CardTitle>
                    <CardDescription>{model.provider}</CardDescription>
                  </div>
                  <Badge variant={model.status === 'healthy' ? 'success' : 'warning'}>
                    {model.status}
                  </Badge>
                </div>
              </CardHeader>
              <CardContent className="flex flex-col gap-3 text-sm text-[var(--color-text-muted)]">
                <p>
                  <span className="font-semibold text-[var(--color-text)]">Upstream:</span>{' '}
                  {model.upstreamModel}
                </p>
                <div className="flex flex-wrap gap-2 pt-1">
                  {model.tags.map((tag) => (
                    <Badge key={tag}>{tag}</Badge>
                  ))}
                </div>
              </CardContent>
            </Card>
          ))
        )}
      </div>
    </div>
  )
}
