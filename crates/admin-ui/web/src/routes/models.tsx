import { createFileRoute, useRouter } from '@tanstack/react-router'
import { HomeIcon } from '@hugeicons/core-free-icons'

import { BrandIcon } from '@/components/icons/brand-icon'
import { AppIcon } from '@/components/icons/app-icon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
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

const DEFAULT_PAGE = 1
const DEFAULT_PAGE_SIZE = 30

export const Route = createFileRoute('/models')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  validateSearch: (search: Record<string, unknown>) => normalizeModelsSearch(search),
  loaderDeps: ({ search }) => search,
  loader: ({ deps }) => getModels({ data: deps }),
  component: ModelsPage,
})

function ModelsPage() {
  const { data: modelPage } = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()
  const totalPages = Math.max(1, Math.ceil(modelPage.total / modelPage.page_size))

  function navigateToPage(page: number) {
    void router.navigate({
      to: '/models',
      search: normalizeModelsSearch({
        ...search,
        page,
        page_size: search.page_size,
      }),
    })
  }

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Models</CardTitle>
          <CardDescription>
            Review routed models, upstream targets, and current health status.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="flex flex-wrap items-center justify-between gap-3 text-sm text-[var(--color-text-muted)]">
            <span>
              Showing {modelPage.items.length} of {modelPage.total} models
            </span>
            <span>
              Page {modelPage.page} of {totalPages}
            </span>
          </div>

          <div className="grid gap-4 lg:grid-cols-2">
            {modelPage.items.length === 0 ? (
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
              modelPage.items.map((model) => (
                <Card key={model.id}>
                  <CardHeader className="gap-4">
                    <div className="flex items-start justify-between gap-3">
                      <div className="flex min-w-0 items-start gap-3">
                        <BrandIcon iconKey={model.model_icon_key} size={20} className="mt-0.5" />
                        <div className="flex min-w-0 flex-col gap-1">
                          <div className="flex flex-wrap items-center gap-2">
                            <CardTitle>{model.id}</CardTitle>
                            {model.alias_of ? <Badge>{`alias → ${model.alias_of}`}</Badge> : null}
                          </div>
                          <CardDescription className="flex flex-wrap items-center gap-2">
                            <BrandIcon iconKey={model.provider_icon_key} size={14} />
                            <span>
                              {model.provider_label ?? model.provider_key ?? 'Unresolved'}
                            </span>
                            {model.provider_key && model.provider_label !== model.provider_key ? (
                              <span className="font-mono text-xs text-[var(--color-text-soft)]">
                                {model.provider_key}
                              </span>
                            ) : null}
                          </CardDescription>
                        </div>
                      </div>
                      <Badge variant={model.status === 'healthy' ? 'success' : 'warning'}>
                        {model.status}
                      </Badge>
                    </div>
                  </CardHeader>
                  <CardContent className="flex flex-col gap-3 text-sm text-[var(--color-text-muted)]">
                    <p className="flex items-center gap-2">
                      <span className="font-semibold text-[var(--color-text)]">Resolved:</span>
                      <span>{model.resolved_model_key}</span>
                    </p>
                    <p className="flex items-center gap-2">
                      <span className="font-semibold text-[var(--color-text)]">Upstream:</span>
                      <BrandIcon iconKey={model.model_icon_key} size={14} />
                      <span>{model.upstream_model ?? 'Not currently routed'}</span>
                    </p>
                    {model.description ? <p>{model.description}</p> : null}
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

          <div className="flex items-center justify-end gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={() => navigateToPage(modelPage.page - 1)}
              disabled={modelPage.page <= 1}
            >
              Previous
            </Button>
            <Button
              type="button"
              variant="outline"
              onClick={() => navigateToPage(modelPage.page + 1)}
              disabled={modelPage.page >= totalPages}
            >
              Next
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}

function normalizeModelsSearch(search: Record<string, unknown>) {
  const page = Number(search.page)
  const pageSize = Number(search.page_size)

  return {
    page: Number.isFinite(page) && page >= 1 ? Math.floor(page) : DEFAULT_PAGE,
    page_size:
      Number.isFinite(pageSize) && pageSize >= 1 ? Math.floor(pageSize) : DEFAULT_PAGE_SIZE,
  }
}
