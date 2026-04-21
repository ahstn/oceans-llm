import type { ReactNode } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import {
  CodeIcon,
  Copy01Icon,
  HomeIcon,
  LiveStreaming03Icon,
  ToolsIcon,
  VisionIcon,
  AttachmentIcon,
} from '@hugeicons/core-free-icons'
import { toast } from 'sonner'

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
import type { ModelView } from '@/types/api'

const DEFAULT_PAGE = 1
const DEFAULT_PAGE_SIZE = 30

const CURRENCY_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 4,
})

const COMPACT_NUMBER_FORMATTER = new Intl.NumberFormat('en-US', {
  maximumFractionDigits: 2,
})

export const Route = createFileRoute('/models')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  validateSearch: (search: Record<string, unknown>) => normalizeModelsSearch(search),
  loaderDeps: ({ search }) => search,
  loader: ({ deps }) => getModels({ data: deps }),
  component: ModelsPage,
})

export function ModelsPage() {
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

  async function handleCopy(modelId: string) {
    try {
      await navigator.clipboard.writeText(modelId)
      toast.success('Model ID copied')
    } catch {
      toast.error('Clipboard access failed')
    }
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

          {modelPage.items.length === 0 ? (
            <Card>
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
            <>
              <div className="grid gap-4 md:hidden" data-testid="models-mobile-list">
                {modelPage.items.map((model) => (
                  <ModelCard key={model.id} model={model} onCopy={handleCopy} />
                ))}
              </div>

              <div
                className="hidden overflow-hidden rounded-md border border-[color:var(--color-border)] md:block"
                data-testid="models-desktop-table"
              >
                <table className="w-full text-left text-sm">
                  <thead className="bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]">
                    <tr>
                      <th className="px-3 py-2 font-semibold">Model ID</th>
                      <th className="px-3 py-2 font-semibold">Resolved</th>
                      <th className="px-3 py-2 font-semibold">Provider Type</th>
                      <th className="px-3 py-2 font-semibold">Provider ID</th>
                      <th className="px-3 py-2 font-semibold">Upstream Model</th>
                      <th className="px-3 py-2 font-semibold">Cost / 1M Tokens</th>
                      <th className="px-3 py-2 font-semibold">Context Window</th>
                      <th className="px-3 py-2 font-semibold">Capabilities</th>
                      <th className="px-3 py-2 font-semibold">Status</th>
                      <th className="px-3 py-2 font-semibold">Notes</th>
                    </tr>
                  </thead>
                  <tbody>
                    {modelPage.items.map((model) => (
                      <tr
                        key={model.id}
                        className="border-t border-[color:var(--color-border)] align-top"
                      >
                        <td className="px-3 py-3">
                          <div className="flex min-w-[220px] flex-col gap-2">
                            <div className="flex items-start gap-2">
                              <BrandIcon
                                iconKey={model.model_icon_key}
                                size={16}
                                className="mt-0.5"
                              />
                              <div className="flex min-w-0 flex-col gap-1">
                                <span className="font-semibold text-[var(--color-text)]">
                                  {model.id}
                                </span>
                                <div className="flex flex-wrap items-center gap-2">
                                  <Button
                                    type="button"
                                    size="icon-xs"
                                    variant="ghost"
                                    aria-label={`Copy model ID ${model.id}`}
                                    onClick={() => handleCopy(model.id)}
                                  >
                                    <AppIcon icon={Copy01Icon} size={14} stroke={1.5} />
                                  </Button>
                                  {model.alias_of ? <Badge>{`alias → ${model.alias_of}`}</Badge> : null}
                                </div>
                              </div>
                            </div>
                          </div>
                        </td>
                        <td className="px-3 py-3 text-[var(--color-text-muted)]">
                          {model.resolved_model_key}
                        </td>
                        <td className="px-3 py-3">
                          <div className="flex min-w-[160px] items-center gap-2 text-[var(--color-text)]">
                            <BrandIcon iconKey={model.provider_icon_key} size={14} />
                            <span>{providerTypeLabel(model)}</span>
                          </div>
                        </td>
                        <td className="px-3 py-3 font-mono text-xs text-[var(--color-text-soft)]">
                          {model.provider_key ?? '—'}
                        </td>
                        <td className="px-3 py-3">
                          <div className="flex min-w-[180px] items-center gap-2 text-[var(--color-text-muted)]">
                            <BrandIcon iconKey={model.model_icon_key} size={14} />
                            <span>{model.upstream_model ?? 'Not currently routed'}</span>
                          </div>
                        </td>
                        <td className="px-3 py-3">
                          <StackedMetric
                            topLabel="Input"
                            topValue={formatCost(model.input_cost_per_million_tokens_usd_10000)}
                            bottomLabel="Output"
                            bottomValue={formatCost(model.output_cost_per_million_tokens_usd_10000)}
                          />
                        </td>
                        <td className="px-3 py-3">
                          <StackedMetric
                            topLabel="Input"
                            topValue={formatWindow(model.input_window_tokens ?? model.context_window_tokens)}
                            bottomLabel="Output"
                            bottomValue={formatWindow(model.output_window_tokens)}
                          />
                        </td>
                        <td className="px-3 py-3">
                          <CapabilityBadges model={model} />
                        </td>
                        <td className="px-3 py-3">
                          <Badge variant={model.status === 'healthy' ? 'success' : 'warning'}>
                            {model.status}
                          </Badge>
                        </td>
                        <td className="px-3 py-3">
                          <ModelNotes model={model} />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </>
          )}

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

function ModelCard({ model, onCopy }: { model: ModelView; onCopy: (modelId: string) => void }) {
  return (
    <Card>
      <CardHeader className="gap-4">
        <div className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 items-start gap-3">
            <BrandIcon iconKey={model.model_icon_key} size={20} className="mt-0.5" />
            <div className="flex min-w-0 flex-col gap-2">
              <div className="flex flex-wrap items-center gap-2">
                <CardTitle>{model.id}</CardTitle>
                <Button
                  type="button"
                  size="icon-xs"
                  variant="ghost"
                  aria-label={`Copy model ID ${model.id}`}
                  onClick={() => onCopy(model.id)}
                >
                  <AppIcon icon={Copy01Icon} size={14} stroke={1.5} />
                </Button>
                {model.alias_of ? <Badge>{`alias → ${model.alias_of}`}</Badge> : null}
              </div>
              <CardDescription className="flex flex-wrap items-center gap-2">
                <BrandIcon iconKey={model.provider_icon_key} size={14} />
                <span>{providerTypeLabel(model)}</span>
              </CardDescription>
            </div>
          </div>
          <Badge variant={model.status === 'healthy' ? 'success' : 'warning'}>{model.status}</Badge>
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-4 text-sm">
        <dl className="grid grid-cols-2 gap-x-4 gap-y-3 text-sm">
          <MetricDetail label="Resolved" value={model.resolved_model_key} />
          <MetricDetail label="Provider ID" value={model.provider_key ?? '—'} mono />
          <MetricDetail label="Upstream" value={model.upstream_model ?? 'Not currently routed'} />
          <MetricDetail
            label="Cost / 1M"
            value={
              <>
                <StackedMetric
                  topLabel="Input"
                  topValue={formatCost(model.input_cost_per_million_tokens_usd_10000)}
                  bottomLabel="Output"
                  bottomValue={formatCost(model.output_cost_per_million_tokens_usd_10000)}
                />
              </>
            }
          />
          <MetricDetail
            label="Context Window"
            value={
              <>
                <StackedMetric
                  topLabel="Input"
                  topValue={formatWindow(model.input_window_tokens ?? model.context_window_tokens)}
                  bottomLabel="Output"
                  bottomValue={formatWindow(model.output_window_tokens)}
                />
              </>
            }
          />
          <MetricDetail
            label="Capabilities"
            value={
              <>
                <CapabilityBadges model={model} />
              </>
            }
          />
        </dl>
        <ModelNotes model={model} />
      </CardContent>
    </Card>
  )
}

function MetricDetail({
  label,
  mono = false,
  value,
}: {
  label: string
  mono?: boolean
  value: ReactNode
}) {
  return (
    <div>
      <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
        {label}
      </dt>
      <dd
        className={mono ? 'font-mono text-xs text-[var(--color-text-muted)]' : 'text-[var(--color-text-muted)]'}
      >
        {value}
      </dd>
    </div>
  )
}

function ModelNotes({ model }: { model: ModelView }) {
  if (!model.description && model.tags.length === 0) {
    return <span className="text-[var(--color-text-soft)]">—</span>
  }

  return (
    <div className="flex min-w-[180px] flex-col gap-2">
      {model.description ? <p className="text-[var(--color-text-muted)]">{model.description}</p> : null}
      {model.tags.length > 0 ? (
        <div className="flex flex-wrap gap-2">
          {model.tags.map((tag) => (
            <Badge key={tag} variant="outline">
              {tag}
            </Badge>
          ))}
        </div>
      ) : null}
    </div>
  )
}

function StackedMetric({
  topLabel,
  topValue,
  bottomLabel,
  bottomValue,
}: {
  topLabel: string
  topValue: string
  bottomLabel: string
  bottomValue: string
}) {
  return (
    <div className="flex min-w-[120px] flex-col gap-1">
      <div className="flex items-center justify-between gap-3">
        <span className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
          {topLabel}
        </span>
        <span className="text-[var(--color-text-muted)]">{topValue}</span>
      </div>
      <div className="flex items-center justify-between gap-3">
        <span className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
          {bottomLabel}
        </span>
        <span className="text-[var(--color-text-muted)]">{bottomValue}</span>
      </div>
    </div>
  )
}

function CapabilityBadges({ model }: { model: ModelView }) {
  const capabilities = [
    model.supports_streaming
      ? {
          label: 'Streaming',
          icon: LiveStreaming03Icon,
        }
      : null,
    model.supports_vision
      ? {
          label: 'Vision',
          icon: VisionIcon,
        }
      : null,
    model.supports_tool_calling
      ? {
          label: 'Tool Calling',
          icon: ToolsIcon,
        }
      : null,
    model.supports_structured_output
      ? {
          label: 'Structured Output',
          icon: CodeIcon,
        }
      : null,
    model.supports_attachments
      ? {
          label: 'Attachments',
          icon: AttachmentIcon,
        }
      : null,
  ].filter(
    (
      value,
    ): value is {
      label: string
      icon: typeof LiveStreaming03Icon
    } => value !== null,
  )

  if (capabilities.length === 0) {
    return <span className="text-[var(--color-text-soft)]">—</span>
  }

  return (
    <div className="flex min-w-[170px] flex-wrap gap-2">
      {capabilities.map((capability) => (
        <Badge key={capability.label} variant="outline" className="gap-1.5">
          <AppIcon icon={capability.icon} size={12} stroke={1.5} />
          {capability.label}
        </Badge>
      ))}
    </div>
  )
}

function providerTypeLabel(model: ModelView) {
  return model.provider_label ?? model.provider_key ?? 'Unresolved'
}

function formatCost(value: number | null | undefined) {
  if (value == null) {
    return '—'
  }

  return CURRENCY_FORMATTER.format(value / 10_000)
}

function formatWindow(value: number | null | undefined) {
  if (value == null) {
    return '—'
  }

  if (value >= 1_000_000) {
    return `${COMPACT_NUMBER_FORMATTER.format(value / 1_000_000)}M`
  }

  if (value >= 1_000) {
    return `${COMPACT_NUMBER_FORMATTER.format(value / 1_000)}k`
  }

  return String(value)
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
