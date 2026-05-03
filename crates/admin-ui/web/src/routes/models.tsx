import { useState, type ReactNode } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import {
  AttachmentIcon,
  CodeIcon,
  Copy01Icon,
  HomeIcon,
  LiveStreaming03Icon,
  ToolsIcon,
  VisionIcon,
} from '@hugeicons/core-free-icons'
import { toast } from 'sonner'

import { BrandIcon } from '@/components/icons/brand-icon'
import { AppIcon } from '@/components/icons/app-icon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
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
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
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
  const [configDialog, setConfigDialog] = useState<{
    model: ModelView
    activeKey: string
  } | null>(null)
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

  async function handleCopyValue(value: string, successMessage: string) {
    try {
      await navigator.clipboard.writeText(value)
      toast.success(successMessage)
    } catch {
      toast.error('Clipboard access failed')
    }
  }

  function openClientConfig(model: ModelView) {
    const firstConfig = model.client_configurations[0]
    if (!firstConfig) {
      return
    }
    setConfigDialog({ model, activeKey: firstConfig.key })
  }

  const activeClientConfig =
    configDialog?.model.client_configurations.find((config) => config.key === configDialog.activeKey) ??
    configDialog?.model.client_configurations[0] ??
    null

  return (
    <div className="flex min-w-0 flex-col gap-4">
      <Card className="min-w-0">
        <CardHeader>
          <CardTitle>Models</CardTitle>
          <CardDescription>
            Review routed models, upstream targets, and current health status.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex min-w-0 flex-col gap-4">
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
                  <ModelCard
                    key={model.id}
                    model={model}
                    onCopy={(modelId) => handleCopyValue(modelId, 'Model ID copied')}
                    onOpenClientConfig={openClientConfig}
                  />
                ))}
              </div>

              <div className="hidden min-w-0 md:block" data-testid="models-desktop-table">
                <Table className="min-w-[92rem] table-fixed">
                  <TableHeader>
                    <TableRow>
                      <TableHead className="bg-card sticky left-0 z-20 w-[16rem] min-w-[16rem] px-4">
                        Model ID
                      </TableHead>
                      <TableHead className="w-[16rem] px-4">Upstream Model</TableHead>
                      <TableHead className="w-[16rem] px-4">Provider</TableHead>
                      <TableHead className="w-[12rem] px-4">Cost / 1M Tokens</TableHead>
                      <TableHead className="w-[12rem] px-4">Context Window</TableHead>
                      <TableHead className="w-[18rem] px-4">Capabilities</TableHead>
                      <TableHead className="w-[10rem] px-4">Client Config</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {modelPage.items.map((model) => (
                      <TableRow key={model.id} className="align-top">
                        <TableCell
                          className="bg-card sticky left-0 z-10 px-4 shadow-[8px_0_12px_-12px_rgba(0,0,0,0.8)]"
                          data-testid={`models-desktop-cell-${model.id}`}
                        >
                          <div className="flex min-w-0 flex-col gap-2 py-1">
                            <div className="flex min-w-0 items-start gap-3">
                              <BrandIcon
                                iconKey={model.model_icon_key}
                                size={18}
                                className="mt-0.5 shrink-0"
                              />
                              <div className="flex min-w-0 flex-col gap-2">
                                <div className="flex min-w-0 items-center gap-2">
                                  <span className="truncate font-semibold text-[var(--color-text)]">
                                    {model.id}
                                  </span>
                                  <ModelStatusIndicator status={model.status} />
                                  <Button
                                    type="button"
                                    size="icon-xs"
                                    variant="ghost"
                                    className="shrink-0"
                                    aria-label={`Copy model ID ${model.id}`}
                                    onClick={() => handleCopyValue(model.id, 'Model ID copied')}
                                  >
                                    <AppIcon icon={Copy01Icon} size={14} stroke={1.5} />
                                  </Button>
                                </div>
                                {model.alias_of ? (
                                  <div>
                                    <Badge variant="secondary">{`alias → ${model.alias_of}`}</Badge>
                                  </div>
                                ) : null}
                              </div>
                            </div>
                          </div>
                        </TableCell>
                        <TableCell className="px-4">
                          <div className="flex min-w-0 flex-col gap-2 py-1">
                            <div className="flex min-w-0 items-center gap-2">
                              <BrandIcon iconKey={model.model_icon_key} size={14} />
                              <span className="truncate text-[var(--color-text)]">
                                {model.upstream_model ?? 'Not currently routed'}
                              </span>
                            </div>
                          </div>
                        </TableCell>
                        <TableCell className="px-4">
                          <div className="flex min-w-0 flex-col gap-2 py-1">
                            <div className="flex min-w-0 items-center gap-2">
                              <BrandIcon iconKey={model.provider_icon_key} size={14} />
                              <span className="truncate text-[var(--color-text)]">
                                {providerTypeLabel(model)}
                              </span>
                            </div>
                            {model.provider_key && model.provider_label !== model.provider_key ? (
                              <span className="truncate font-mono text-xs text-[var(--color-text-soft)]">
                                {model.provider_key}
                              </span>
                            ) : null}
                          </div>
                        </TableCell>
                        <TableCell className="px-4 whitespace-normal">
                          <StackedMetric
                            topLabel="Input"
                            topValue={formatCost(model.input_cost_per_million_tokens_usd_10000)}
                            bottomLabel="Output"
                            bottomValue={formatCost(model.output_cost_per_million_tokens_usd_10000)}
                          />
                        </TableCell>
                        <TableCell className="px-4 whitespace-normal">
                          <StackedMetric
                            topLabel="Input"
                            topValue={formatWindow(
                              model.input_window_tokens ?? model.context_window_tokens,
                            )}
                            bottomLabel="Output"
                            bottomValue={formatWindow(model.output_window_tokens)}
                          />
                        </TableCell>
                        <TableCell className="px-4 whitespace-normal">
                          <CapabilityBadges model={model} />
                        </TableCell>
                        <TableCell className="px-4 whitespace-normal">
                          <ClientConfigButton model={model} onOpen={openClientConfig} />
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
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

      <ClientConfigDialog
        model={configDialog?.model ?? null}
        activeKey={configDialog?.activeKey ?? null}
        activeConfig={activeClientConfig}
        onActiveKeyChange={(activeKey) =>
          setConfigDialog((current) => (current ? { ...current, activeKey } : current))
        }
        onCopy={(content) => handleCopyValue(content, 'Client config copied')}
        onOpenChange={(open) => {
          if (!open) {
            setConfigDialog(null)
          }
        }}
      />
    </div>
  )
}

function ModelCard({
  model,
  onCopy,
  onOpenClientConfig,
}: {
  model: ModelView
  onCopy: (modelId: string) => void
  onOpenClientConfig: (model: ModelView) => void
}) {
  return (
    <Card>
      <CardHeader className="gap-4">
        <div className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 items-start gap-3">
            <BrandIcon iconKey={model.model_icon_key} size={20} className="mt-0.5" />
            <div className="flex min-w-0 flex-col gap-2">
              <div className="flex flex-wrap items-center gap-2">
                <CardTitle>{model.id}</CardTitle>
                <ModelStatusIndicator status={model.status} />
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
              <StackedMetric
                topLabel="Input"
                topValue={formatCost(model.input_cost_per_million_tokens_usd_10000)}
                bottomLabel="Output"
                bottomValue={formatCost(model.output_cost_per_million_tokens_usd_10000)}
              />
            }
          />
          <MetricDetail
            label="Context Window"
            value={
              <StackedMetric
                topLabel="Input"
                topValue={formatWindow(model.input_window_tokens ?? model.context_window_tokens)}
                bottomLabel="Output"
                bottomValue={formatWindow(model.output_window_tokens)}
              />
            }
          />
          <MetricDetail label="Capabilities" value={<CapabilityBadges model={model} />} />
        </dl>
        <ModelNotes model={model} />
        <ClientConfigButton model={model} onOpen={onOpenClientConfig} />
      </CardContent>
    </Card>
  )
}

function ClientConfigButton({
  model,
  onOpen,
}: {
  model: ModelView
  onOpen: (model: ModelView) => void
}) {
  if (model.client_configurations.length === 0) {
    return <span className="text-[var(--color-text-soft)]">—</span>
  }

  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      className="gap-2"
      onClick={() => onOpen(model)}
    >
      <AppIcon icon={CodeIcon} size={14} stroke={1.5} />
      Client config
    </Button>
  )
}

function ClientConfigDialog({
  model,
  activeKey,
  activeConfig,
  onActiveKeyChange,
  onCopy,
  onOpenChange,
}: {
  model: ModelView | null
  activeKey: string | null
  activeConfig: ModelView['client_configurations'][number] | null
  onActiveKeyChange: (key: string) => void
  onCopy: (content: string) => void
  onOpenChange: (open: boolean) => void
}) {
  return (
    <Dialog open={model !== null} onOpenChange={onOpenChange}>
      <DialogContent className="w-[min(920px,calc(100vw-32px))]">
        <DialogHeader>
          <DialogTitle>Client config</DialogTitle>
          <DialogDescription>
            {model ? `${model.id} via ${providerTypeLabel(model)}` : 'Local client configuration'}
          </DialogDescription>
        </DialogHeader>

        {model && activeConfig ? (
          <div className="flex min-w-0 flex-col gap-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <ToggleGroup
                type="single"
                value={activeKey ?? activeConfig.key}
                onValueChange={(value) => {
                  if (value) {
                    onActiveKeyChange(value)
                  }
                }}
                variant="outline"
                size="sm"
                spacing={0}
                aria-label="Client config"
              >
                {model.client_configurations.map((config) => (
                  <ToggleGroupItem key={config.key} value={config.key} aria-label={config.label}>
                    {config.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
              <Button type="button" variant="outline" size="sm" onClick={() => onCopy(activeConfig.content)}>
                Copy JSON
              </Button>
            </div>

            <div className="flex flex-wrap items-center gap-2 text-sm text-[var(--color-text-muted)]">
              <Badge variant="secondary">{activeConfig.filename}</Badge>
              <span>{model.upstream_model ?? model.resolved_model_key}</span>
            </div>

            <pre className="max-h-[460px] min-h-[280px] overflow-auto rounded-md border bg-[var(--color-surface-muted)] p-4 text-xs leading-6 text-[var(--color-text-muted)]">
              <code>{activeConfig.content}</code>
            </pre>

            {activeConfig.notes.length > 0 ? (
              <div className="flex flex-col gap-2 text-sm text-[var(--color-text-muted)]">
                {activeConfig.notes.map((note) => (
                  <p key={note}>{note}</p>
                ))}
              </div>
            ) : null}
          </div>
        ) : null}
      </DialogContent>
    </Dialog>
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
        className={
          mono
            ? 'font-mono text-xs text-[var(--color-text-muted)]'
            : 'text-[var(--color-text-muted)]'
        }
      >
        {value}
      </dd>
    </div>
  )
}

function ModelStatusIndicator({ status }: { status: string }) {
  const tone =
    status === 'healthy' ? 'bg-emerald-500 shadow-emerald-500/30' : 'bg-amber-400 shadow-amber-400/30'

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          aria-label={status}
          className={`inline-flex size-2.5 shrink-0 rounded-full shadow-[0_0_0_3px] ${tone}`}
        />
      </TooltipTrigger>
      <TooltipContent sideOffset={6}>{status}</TooltipContent>
    </Tooltip>
  )
}

function ModelNotes({ model }: { model: ModelView }) {
  if (!model.description && model.tags.length === 0) {
    return <span className="text-[var(--color-text-soft)]">—</span>
  }

  return (
    <div className="flex min-w-0 flex-col gap-2 py-1">
      {model.description ? (
        <p className="line-clamp-2 whitespace-normal text-[var(--color-text-muted)]">
          {model.description}
        </p>
      ) : null}
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
    <div className="flex min-w-[10rem] flex-col gap-1 py-1">
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
    model.supports_streaming ? { label: 'Streaming', icon: LiveStreaming03Icon } : null,
    model.supports_vision ? { label: 'Vision', icon: VisionIcon } : null,
    model.supports_tool_calling ? { label: 'Tool Calling', icon: ToolsIcon } : null,
    model.supports_structured_output ? { label: 'Structured Output', icon: CodeIcon } : null,
    model.supports_attachments ? { label: 'Attachments', icon: AttachmentIcon } : null,
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
    <div className="flex min-w-0 flex-wrap gap-2 py-1">
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
