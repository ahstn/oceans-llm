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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
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
      <Card>
        <CardHeader>
          <CardTitle>Models</CardTitle>
          <CardDescription>
            Review routed models, upstream targets, and current health status.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {data.length === 0 ? (
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
          ) : (
            <>
              <div className="grid gap-3 md:hidden">
                {data.map((model) => (
                  <div key={model.id} className="border-border bg-muted rounded-lg border p-4">
                    <div className="flex items-start justify-between gap-3">
                      <div className="flex flex-col gap-1">
                        <p className="text-foreground font-semibold">{model.id}</p>
                        <p className="text-muted-foreground/80 font-mono text-xs">
                          {model.provider}
                        </p>
                      </div>
                      <Badge variant={model.status === 'healthy' ? 'success' : 'warning'}>
                        {model.status}
                      </Badge>
                    </div>
                    <dl className="mt-4 grid gap-x-4 gap-y-3 text-sm">
                      <div>
                        <dt className="text-muted-foreground/80 text-xs font-semibold tracking-[0.08em] uppercase">
                          Upstream Model
                        </dt>
                        <dd className="text-muted-foreground mt-1 truncate">
                          {model.upstreamModel}
                        </dd>
                      </div>
                      {model.tags.length > 0 && (
                        <div>
                          <dt className="text-muted-foreground/80 text-xs font-semibold tracking-[0.08em] uppercase">
                            Tags
                          </dt>
                          <dd className="mt-1 flex flex-wrap gap-2">
                            {model.tags.map((tag) => (
                              <Badge key={tag}>{tag}</Badge>
                            ))}
                          </dd>
                        </div>
                      )}
                    </dl>
                  </div>
                ))}
              </div>

              <div className="border-border hidden overflow-hidden rounded-md border md:block">
                <Table>
                  <TableHeader className="bg-muted">
                    <TableRow className="hover:bg-muted">
                      <TableHead>Model ID</TableHead>
                      <TableHead>Provider</TableHead>
                      <TableHead>Upstream</TableHead>
                      <TableHead>Tags</TableHead>
                      <TableHead>Status</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {data.map((model) => (
                      <TableRow key={model.id}>
                        <TableCell className="text-foreground font-medium">{model.id}</TableCell>
                        <TableCell className="text-muted-foreground/80 font-mono text-xs">
                          {model.provider}
                        </TableCell>
                        <TableCell className="text-muted-foreground">
                          {model.upstreamModel}
                        </TableCell>
                        <TableCell>
                          <div className="flex flex-wrap gap-1">
                            {model.tags.map((tag) => (
                              <Badge
                                key={tag}
                                variant="default"
                                className="py-0 text-[10px] font-normal"
                              >
                                {tag}
                              </Badge>
                            ))}
                          </div>
                        </TableCell>
                        <TableCell>
                          <Badge variant={model.status === 'healthy' ? 'success' : 'warning'}>
                            {model.status}
                          </Badge>
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
    </div>
  )
}
