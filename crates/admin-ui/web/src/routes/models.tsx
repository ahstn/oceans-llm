import { useState, type ComponentProps, type ReactNode } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import {
  AttachmentIcon,
  BadgeInfoIcon,
  CircleCheckIcon,
  CodeIcon,
  ColumnsThreeCogIcon,
  Copy01Icon,
  HomeIcon,
  LiveStreaming03Icon,
  RefreshIcon,
  ToolsIcon,
  Tick02Icon,
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
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
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
import { cn } from '@/lib/utils'
import { requireAdminSession } from '@/routes/-admin-guard'
import {
  getModelClientConfigs,
  getModels,
  refreshModelPricing,
} from '@/server/admin-data.functions'
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

type ModelInfoSectionKey = 'overview' | 'routing' | 'economics' | 'access'

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
    models: ModelView[]
    activeKey: string
    clientConfigurations: ModelView['client_configurations']
  } | null>(null)
  const [infoDialogModel, setInfoDialogModel] = useState<ModelView | null>(null)
  const [modelInfoSection, setModelInfoSection] = useState<ModelInfoSectionKey>('overview')
  const [selectedModelsById, setSelectedModelsById] = useState<Record<string, ModelView>>({})
  const [visibleColumns, setVisibleColumns] = useState({
    contextWindow: false,
    capabilities: false,
  })
  const [isGeneratingConfig, setIsGeneratingConfig] = useState(false)
  const [isRefreshingPricing, setIsRefreshingPricing] = useState(false)
  const totalPages = Math.max(1, Math.ceil(modelPage.total / modelPage.page_size))
  const selectableModels = modelPage.items.filter((model) => model.client_configurations.length > 0)
  const selectedModels = Object.values(selectedModelsById)
  const selectedModelIds = Object.keys(selectedModelsById)
  const selectedModelIdSet = new Set(selectedModelIds)
  const allSelectableSelected =
    selectableModels.length > 0 &&
    selectableModels.every((model) => selectedModelIdSet.has(model.id))
  const desktopTableMinWidth =
    visibleColumns.contextWindow && visibleColumns.capabilities
      ? 'min-w-[109rem]'
      : visibleColumns.capabilities
        ? 'min-w-[97rem]'
        : visibleColumns.contextWindow
          ? 'min-w-[91rem]'
          : 'min-w-[79rem]'

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

  function toggleModelSelection(model: ModelView) {
    if (model.client_configurations.length === 0) {
      return
    }
    setSelectedModelsById((current) => {
      if (current[model.id]) {
        const { [model.id]: _removed, ...remaining } = current
        return remaining
      }

      return { ...current, [model.id]: model }
    })
  }

  function toggleAllSelectableModels() {
    setSelectedModelsById((current) => {
      if (selectableModels.every((model) => current[model.id])) {
        return Object.fromEntries(
          Object.entries(current).filter(
            ([id]) => !selectableModels.some((model) => model.id === id),
          ),
        )
      }

      return Object.fromEntries([
        ...Object.entries(current),
        ...selectableModels.map((model) => [model.id, model] as const),
      ])
    })
  }

  async function openClientConfig(models: ModelView[]) {
    const modelKeys = models.map((model) => model.id)
    if (modelKeys.length === 0) {
      return
    }
    setIsGeneratingConfig(true)
    try {
      const response = await getModelClientConfigs({ data: { model_keys: modelKeys } })
      const firstConfig = response.data.client_configurations[0]
      if (!firstConfig) {
        toast.error('No client config is available for the selected models')
        return
      }
      setConfigDialog({
        models,
        activeKey: firstConfig.key,
        clientConfigurations: response.data.client_configurations,
      })
    } catch {
      toast.error('Client config generation failed')
    } finally {
      setIsGeneratingConfig(false)
    }
  }

  function openSelectedClientConfig() {
    void openClientConfig(selectedModels)
  }

  function openSingleClientConfig(model: ModelView) {
    void openClientConfig([model])
  }

  async function refreshPricing() {
    setIsRefreshingPricing(true)
    try {
      await refreshModelPricing()
      toast.success('Pricing refreshed')
    } catch {
      toast.error('Pricing refresh failed')
      setIsRefreshingPricing(false)
      return
    }

    try {
      await router.invalidate()
    } catch {
      toast.error('Pricing refreshed, but the model list did not reload')
    } finally {
      setIsRefreshingPricing(false)
    }
  }

  function openModelInfo(model: ModelView) {
    setModelInfoSection('overview')
    setInfoDialogModel(model)
  }

  const activeClientConfig =
    configDialog?.clientConfigurations.find((config) => config.key === configDialog.activeKey) ??
    configDialog?.clientConfigurations[0] ??
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
          <div className="hidden flex-wrap items-center justify-between gap-3 rounded-md border border-[color:var(--color-border)] px-3 py-2 md:flex">
            <span className="text-sm text-[var(--color-text-muted)]">
              {selectedModelIds.length} selected for client config
            </span>
            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => setSelectedModelsById({})}
                disabled={selectedModelIds.length === 0 || isGeneratingConfig}
              >
                Clear
              </Button>
              <Popover>
                <PopoverTrigger asChild>
                  <Button type="button" variant="outline" size="sm" className="gap-2">
                    <AppIcon icon={ColumnsThreeCogIcon} size={14} stroke={1.5} />
                    Columns
                  </Button>
                </PopoverTrigger>
                <PopoverContent align="end" className="w-64 gap-3 p-3">
                  <div className="flex flex-col gap-1">
                    <h2 className="text-sm font-medium text-[var(--color-text)]">Table columns</h2>
                    <p className="text-xs text-[var(--color-text-muted)]">
                      Show secondary model details in the desktop table.
                    </p>
                  </div>
                  <div className="flex flex-col gap-2">
                    <label className="hover:bg-muted/50 flex cursor-pointer items-start gap-3 rounded-md px-1 py-1.5 text-sm">
                      <ModelCheckbox
                        className="mt-0.5"
                        checked={visibleColumns.contextWindow}
                        onChange={(event) => {
                          const checked = event.currentTarget.checked
                          setVisibleColumns((current) => ({
                            ...current,
                            contextWindow: checked,
                          }))
                        }}
                      />
                      <span className="flex min-w-0 flex-col gap-0.5">
                        <span className="font-medium text-[var(--color-text)]">Context window</span>
                        <span className="text-xs text-[var(--color-text-muted)]">
                          Input and output token limits.
                        </span>
                      </span>
                    </label>
                    <label className="hover:bg-muted/50 flex cursor-pointer items-start gap-3 rounded-md px-1 py-1.5 text-sm">
                      <ModelCheckbox
                        className="mt-0.5"
                        checked={visibleColumns.capabilities}
                        onChange={(event) => {
                          const checked = event.currentTarget.checked
                          setVisibleColumns((current) => ({
                            ...current,
                            capabilities: checked,
                          }))
                        }}
                      />
                      <span className="flex min-w-0 flex-col gap-0.5">
                        <span className="font-medium text-[var(--color-text)]">Capabilities</span>
                        <span className="text-xs text-[var(--color-text-muted)]">
                          Streaming, vision, tools, and attachment support.
                        </span>
                      </span>
                    </label>
                  </div>
                </PopoverContent>
              </Popover>
              <Button
                type="button"
                variant="secondary"
                size="sm"
                className="gap-2"
                onClick={openSelectedClientConfig}
                disabled={selectedModelIds.length === 0 || isGeneratingConfig}
              >
                <AppIcon icon={CodeIcon} size={14} stroke={1.5} data-icon="inline-start" />
                Generate config
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="gap-2"
                onClick={() => void refreshPricing()}
                disabled={isRefreshingPricing}
              >
                <AppIcon icon={RefreshIcon} size={14} stroke={1.5} data-icon="inline-start" />
                {isRefreshingPricing ? 'Refreshing...' : 'Refresh pricing'}
              </Button>
            </div>
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
                    onOpenClientConfig={openSingleClientConfig}
                  />
                ))}
              </div>

              <div
                className="hidden min-w-0 overflow-hidden rounded-md border border-[color:var(--color-border)] md:block"
                data-testid="models-desktop-table"
              >
                <Table className={`${desktopTableMinWidth} table-fixed`}>
                  <TableHeader className="bg-[color:var(--color-surface-muted)]">
                    <TableRow>
                      <TableHead className="sticky left-0 z-30 w-[3rem] bg-[color:var(--color-surface-muted)] px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                        <ModelCheckbox
                          aria-label="Select all configurable models"
                          checked={allSelectableSelected}
                          disabled={selectableModels.length === 0}
                          onChange={toggleAllSelectableModels}
                        />
                      </TableHead>
                      <TableHead className="sticky left-[3rem] z-30 w-[16rem] min-w-[16rem] bg-[color:var(--color-surface-muted)] px-3 py-2 font-semibold text-[var(--color-text-soft)] shadow-[8px_0_12px_-12px_rgba(0,0,0,0.8)]">
                        Model id
                      </TableHead>
                      <TableHead className="w-[12rem] px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                        Actions
                      </TableHead>
                      <TableHead className="w-[24rem] px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                        Provider and model
                      </TableHead>
                      <TableHead className="w-[12rem] px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                        Cost / 1M tokens
                      </TableHead>
                      {visibleColumns.contextWindow ? (
                        <TableHead className="w-[12rem] px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                          Context window
                        </TableHead>
                      ) : null}
                      {visibleColumns.capabilities ? (
                        <TableHead className="w-[18rem] px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                          Capabilities
                        </TableHead>
                      ) : null}
                      <TableHead className="w-[12rem] px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                        Model allowlist
                      </TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {modelPage.items.map((model) => (
                      <TableRow key={model.id} className="align-middle">
                        <TableCell className="bg-card sticky left-0 z-20 px-3 py-3">
                          <ModelCheckbox
                            aria-label={`Select model ${model.id}`}
                            checked={selectedModelIdSet.has(model.id)}
                            disabled={model.client_configurations.length === 0}
                            onChange={() => toggleModelSelection(model)}
                          />
                        </TableCell>
                        <TableCell
                          className="bg-card sticky left-[3rem] z-20 px-3 py-3 shadow-[8px_0_12px_-12px_rgba(0,0,0,0.8)]"
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
                        <TableCell className="px-3 py-3 whitespace-normal">
                          <ModelActions
                            model={model}
                            onOpenClientConfig={openSingleClientConfig}
                            onOpenInfo={openModelInfo}
                          />
                        </TableCell>
                        <TableCell className="px-3 py-3">
                          <div className="flex min-w-0 flex-col gap-2 py-1">
                            <div className="flex min-w-0 items-center gap-2">
                              <BrandIcon iconKey={model.model_icon_key} size={14} />
                              <span className="truncate text-[var(--color-text)]">
                                {model.upstream_model ?? 'Not currently routed'}
                              </span>
                            </div>
                            <div className="flex min-w-0 items-center gap-2 truncate text-xs tracking-[0.08em] text-[var(--color-text-soft)]">
                              <BrandIcon
                                iconKey={model.provider_icon_key}
                                size={14}
                                className="shrink-0"
                              />
                              <span className="truncate">{providerTypeLabel(model)}</span>
                            </div>
                          </div>
                        </TableCell>
                        <TableCell className="px-3 py-3 whitespace-normal">
                          <StackedMetric
                            topLabel="Input"
                            topValue={formatCost(model.input_cost_per_million_tokens_usd_10000)}
                            bottomLabel="Output"
                            bottomValue={formatCost(model.output_cost_per_million_tokens_usd_10000)}
                          />
                        </TableCell>
                        {visibleColumns.contextWindow ? (
                          <TableCell className="px-3 py-3 whitespace-normal">
                            <StackedMetric
                              topLabel="Input"
                              topValue={formatWindow(
                                model.input_window_tokens ?? model.context_window_tokens,
                              )}
                              bottomLabel="Output"
                              bottomValue={formatWindow(model.output_window_tokens)}
                            />
                          </TableCell>
                        ) : null}
                        {visibleColumns.capabilities ? (
                          <TableCell className="px-3 py-3 whitespace-normal">
                            <CapabilityBadges model={model} />
                          </TableCell>
                        ) : null}
                        <TableCell className="px-3 py-3 whitespace-normal">
                          <ModelAllowlistDetail model={model} compact />
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
        models={configDialog?.models ?? []}
        activeKey={configDialog?.activeKey ?? null}
        activeConfig={activeClientConfig}
        clientConfigurations={configDialog?.clientConfigurations ?? []}
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
      <ModelInfoDialog
        model={infoDialogModel}
        activeSection={modelInfoSection}
        onActiveSectionChange={setModelInfoSection}
        onOpenChange={(open) => {
          if (!open) {
            setInfoDialogModel(null)
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
          <MetricDetail label="Model allowlist" value={<ModelAllowlistDetail model={model} />} />
        </dl>
        <ModelNotes model={model} />
        <ClientConfigButton model={model} onOpen={onOpenClientConfig} />
      </CardContent>
    </Card>
  )
}

function ModelActions({
  model,
  onOpenClientConfig,
  onOpenInfo,
}: {
  model: ModelView
  onOpenClientConfig: (model: ModelView) => void
  onOpenInfo: (model: ModelView) => void
}) {
  return (
    <div className="flex items-center gap-2">
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            className="gap-2"
            onClick={() => onOpenInfo(model)}
          >
            <AppIcon icon={BadgeInfoIcon} size={14} stroke={1.5} />
            Info
          </Button>
        </TooltipTrigger>
        <TooltipContent sideOffset={6}>Model info</TooltipContent>
      </Tooltip>
      <ClientConfigButton model={model} onOpen={onOpenClientConfig} compact />
    </div>
  )
}

function ModelCheckbox({
  className,
  ...props
}: Omit<ComponentProps<'input'>, 'type' | 'className'> & {
  className?: string
}) {
  return (
    <span className={cn('relative inline-flex size-5 shrink-0', className)}>
      <input
        type="checkbox"
        className="peer checked:border-primary checked:bg-primary focus-visible:border-ring focus-visible:ring-ring/50 size-5 shrink-0 appearance-none rounded-md border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] transition-colors focus-visible:ring-3 disabled:cursor-not-allowed disabled:opacity-50"
        {...props}
      />
      <span
        aria-hidden="true"
        className="text-primary-foreground pointer-events-none absolute inset-0 flex items-center justify-center opacity-0 transition-opacity peer-checked:opacity-100"
      >
        <AppIcon icon={Tick02Icon} size={14} stroke={2.5} />
      </span>
    </span>
  )
}

function ClientConfigButton({
  compact = false,
  model,
  onOpen,
}: {
  compact?: boolean
  model: ModelView
  onOpen: (model: ModelView) => void
}) {
  if (model.client_configurations.length === 0) {
    return <span className="text-[var(--color-text-soft)]">—</span>
  }

  const label = `Generate client config for ${model.id}`

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant={compact ? 'secondary' : 'outline'}
          size="sm"
          className="gap-2"
          aria-label={compact ? label : undefined}
          onClick={() => onOpen(model)}
        >
          <AppIcon icon={CodeIcon} size={14} stroke={1.5} />
          {compact ? 'Config' : 'Client config'}
        </Button>
      </TooltipTrigger>
      <TooltipContent sideOffset={6}>{label}</TooltipContent>
    </Tooltip>
  )
}

function ModelInfoDialog({
  model,
  activeSection,
  onActiveSectionChange,
  onOpenChange,
}: {
  model: ModelView | null
  activeSection: ModelInfoSectionKey
  onActiveSectionChange: (section: ModelInfoSectionKey) => void
  onOpenChange: (open: boolean) => void
}) {
  const sections: Array<{ key: ModelInfoSectionKey; label: string }> = [
    { key: 'overview', label: 'Overview' },
    { key: 'routing', label: 'Routing' },
    { key: 'economics', label: 'Economics' },
    { key: 'access', label: 'Access' },
  ]

  const activeLabel = sections.find((section) => section.key === activeSection)?.label ?? 'Overview'

  return (
    <Dialog open={model !== null} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[calc(100dvh-2rem)] w-[min(1080px,calc(100vw-32px))] max-w-[calc(100vw-2rem)] flex-col overflow-hidden sm:max-h-[82vh] sm:max-w-[min(1080px,calc(100vw-2rem))]">
        {model ? (
          <>
            <DialogHeader>
              <DialogTitle>Model info</DialogTitle>
              <DialogDescription className="flex min-w-0 flex-wrap items-center gap-2">
                <BrandIcon iconKey={model.model_icon_key} size={14} />
                <span className="truncate font-mono text-xs">{model.id}</span>
                <span>via {providerTypeLabel(model)}</span>
              </DialogDescription>
            </DialogHeader>

            <div className="flex min-h-0 flex-1 flex-col overflow-hidden border-t">
              <nav
                aria-label="Model info sections"
                className="flex gap-3 overflow-x-auto border-b py-3"
              >
                {sections.map((section) => (
                  <Button
                    key={section.key}
                    type="button"
                    variant={activeSection === section.key ? 'secondary' : 'ghost'}
                    size="sm"
                    className="justify-start px-3"
                    onClick={() => onActiveSectionChange(section.key)}
                  >
                    {section.label}
                  </Button>
                ))}
              </nav>

              <div className="min-w-0 overflow-y-auto py-5">
                <div className="flex min-w-0 flex-col gap-4">
                  <div>
                    <h3 className="text-sm font-medium text-[var(--color-text)]">{activeLabel}</h3>
                    <p className="mt-1 text-sm text-[var(--color-text-muted)]">
                      {modelInfoSectionDescription(activeSection)}
                    </p>
                  </div>

                  {activeSection === 'overview' ? <ModelInfoOverview model={model} /> : null}
                  {activeSection === 'routing' ? <ModelInfoRouting model={model} /> : null}
                  {activeSection === 'economics' ? <ModelInfoEconomics model={model} /> : null}
                  {activeSection === 'access' ? <ModelInfoAccess model={model} /> : null}
                </div>
              </div>
            </div>
          </>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

function ModelInfoOverview({ model }: { model: ModelView }) {
  return (
    <div className="divide-y">
      <ModelInfoRow label="Gateway model" value={model.id} mono />
      <ModelInfoRow label="Resolved model" value={model.resolved_model_key} mono />
      <ModelInfoRow label="Status" value={<ModelStatusIndicator status={model.status} />} />
      <ModelInfoRow label="Alias of" value={model.alias_of ?? '—'} mono={model.alias_of != null} />
      <ModelInfoRow label="Description" value={model.description ?? '—'} />
      <ModelInfoRow
        label="Tags"
        value={
          model.tags.length > 0 ? (
            <div className="flex flex-wrap gap-2">
              {model.tags.map((tag) => (
                <Badge key={tag} variant="outline">
                  {tag}
                </Badge>
              ))}
            </div>
          ) : (
            '—'
          )
        }
      />
    </div>
  )
}

function ModelInfoRouting({ model }: { model: ModelView }) {
  return (
    <div className="divide-y">
      <ModelInfoRow
        label="Upstream model"
        value={model.upstream_model ?? 'Not currently routed'}
        mono={model.upstream_model != null}
      />
      <ModelInfoRow label="Model ID" value={model.model_id} mono />
      <ModelInfoRow label="Provider" value={providerTypeLabel(model)} />
      <ModelInfoRow label="Provider key" value={model.provider_key ?? '—'} mono />
      <ModelInfoRow
        label="Client config"
        value={
          model.client_configurations.length > 0
            ? `${model.client_configurations.length} available`
            : 'Not available'
        }
      />
    </div>
  )
}

function ModelInfoEconomics({ model }: { model: ModelView }) {
  return (
    <div className="divide-y">
      <ModelInfoRow
        label="Input cost"
        value={formatCost(model.input_cost_per_million_tokens_usd_10000)}
      />
      <ModelInfoRow
        label="Output cost"
        value={formatCost(model.output_cost_per_million_tokens_usd_10000)}
      />
      <ModelInfoRow
        label="Cache read cost"
        value={formatCost(model.cache_read_cost_per_million_tokens_usd_10000)}
      />
      <ModelInfoRow
        label="Input window"
        value={formatWindow(model.input_window_tokens ?? model.context_window_tokens)}
      />
      <ModelInfoRow label="Output window" value={formatWindow(model.output_window_tokens)} />
      <ModelInfoRow label="Context window" value={formatWindow(model.context_window_tokens)} />
    </div>
  )
}

function ModelInfoAccess({ model }: { model: ModelView }) {
  return (
    <div className="divide-y">
      <ModelInfoRow label="Model allowlist" value={<ModelAllowlistDetail model={model} />} />
      <ModelInfoRow label="Capabilities" value={<CapabilityBadges model={model} />} />
    </div>
  )
}

function ModelInfoRow({
  label,
  mono = false,
  value,
}: {
  label: string
  mono?: boolean
  value: ReactNode
}) {
  return (
    <div className="grid min-w-0 gap-2 py-3 text-sm sm:grid-cols-[14rem_minmax(0,1fr)]">
      <dt className="text-[var(--color-text-soft)]">{label}</dt>
      <dd
        className={
          mono
            ? 'min-w-0 font-mono text-xs break-words text-[var(--color-text-muted)]'
            : 'min-w-0 text-[var(--color-text-muted)]'
        }
      >
        {value}
      </dd>
    </div>
  )
}

function modelInfoSectionDescription(section: ModelInfoSectionKey) {
  switch (section) {
    case 'overview':
      return 'Identity, lifecycle state, tags, and operator-facing description.'
    case 'routing':
      return 'Gateway and upstream identifiers used to route requests.'
    case 'economics':
      return 'Token pricing and context limits exposed by the current route.'
    case 'access':
      return 'Allowlist and runtime capability metadata for this model.'
  }
}

function ClientConfigDialog({
  models,
  activeKey,
  activeConfig,
  clientConfigurations,
  onActiveKeyChange,
  onCopy,
  onOpenChange,
}: {
  models: ModelView[]
  activeKey: string | null
  activeConfig: ModelView['client_configurations'][number] | null
  clientConfigurations: ModelView['client_configurations']
  onActiveKeyChange: (key: string) => void
  onCopy: (content: string) => void
  onOpenChange: (open: boolean) => void
}) {
  const isOpen = models.length > 0
  const firstModel = models[0] ?? null
  const description =
    models.length === 1 && firstModel
      ? `${firstModel.id} via ${providerTypeLabel(firstModel)}`
      : `${models.length} selected models`
  const activeModelCount = activeConfig?.model_ids.length ?? 0
  const activeModelSummary =
    activeModelCount === 1
      ? activeConfig?.model_ids[0]
      : activeModelCount > 1
        ? `${activeModelCount} models`
        : 'No applicable models'

  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[calc(100vw-2rem)] sm:max-w-[min(920px,calc(100vw-2rem))] md:min-w-[35vw]">
        <DialogHeader>
          <DialogTitle>Client config</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>

        {isOpen && activeConfig ? (
          <div className="flex min-w-0 flex-col gap-4">
            <div className="flex flex-wrap gap-2">
              {models.map((model) => (
                <Badge key={model.id} variant="secondary">
                  {model.id}
                </Badge>
              ))}
            </div>
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
                {clientConfigurations.map((config) => (
                  <ToggleGroupItem key={config.key} value={config.key} aria-label={config.label}>
                    {config.label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </div>

            {activeConfig.setup.length > 0 ? (
              <Table aria-label={`${activeConfig.label} setup`}>
                <TableBody>
                  {activeConfig.setup.map((item) => (
                    <TableRow key={`${item.label}:${item.value}`}>
                      <TableCell className="w-32 align-baseline font-medium whitespace-nowrap">
                        {item.label}
                      </TableCell>
                      <TableCell className="text-muted-foreground min-w-0 align-baseline whitespace-normal">
                        {item.href ? (
                          <a
                            href={item.href}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="font-mono text-xs break-words underline underline-offset-4"
                          >
                            {item.value}
                          </a>
                        ) : (
                          <span className="break-words">{item.value}</span>
                        )}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            ) : null}

            <div className="flex min-w-0 flex-col gap-4">
              {activeConfig.blocks.map((block) => (
                <div
                  key={`${block.label}:${block.filename}`}
                  className="flex min-w-0 flex-col gap-3"
                >
                  <div className="text-muted-foreground flex flex-wrap items-center justify-between gap-3 text-sm">
                    <div className="flex min-w-0 flex-wrap items-center gap-2">
                      <Badge variant="secondary">{block.filename}</Badge>
                      {block.label !== block.filename ? <span>{block.label}</span> : null}
                      <span>{activeModelSummary}</span>
                    </div>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => onCopy(block.content)}
                    >
                      {copyConfigLabel(block.filename)}
                    </Button>
                  </div>

                  <pre className="bg-muted text-muted-foreground max-h-[min(42vh,420px)] min-h-[220px] overflow-auto rounded-md border p-4 text-xs leading-6">
                    <code>{block.content}</code>
                  </pre>
                </div>
              ))}
            </div>

            {activeConfig.notes.length > 0 ? (
              <div className="text-muted-foreground flex flex-col gap-2 text-sm">
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

function copyConfigLabel(filename: string) {
  if (filename.endsWith('.json')) {
    return 'Copy JSON'
  }
  if (filename.endsWith('.toml')) {
    return 'Copy TOML'
  }
  return 'Copy config'
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
    status === 'healthy'
      ? 'bg-emerald-500 shadow-emerald-500/30'
      : 'bg-amber-400 shadow-amber-400/30'

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

function ModelAllowlistDetail({ compact = false, model }: { compact?: boolean; model: ModelView }) {
  if (!model.allowlist) {
    return (
      <span
        className={`inline-flex items-center gap-1.5 text-[var(--color-text-soft)] ${
          compact ? 'text-sm' : ''
        }`}
      >
        <AppIcon icon={CircleCheckIcon} size={compact ? 13 : 14} stroke={1.5} />
        Unrestricted
      </span>
    )
  }

  const refs = [
    { label: 'Users', values: model.allowlist.users },
    { label: 'Teams', values: model.allowlist.teams },
  ].filter((entry) => entry.values.length > 0)

  if (refs.length === 0) {
    return <span className="text-[var(--color-text-soft)]">No users or teams listed</span>
  }

  return (
    <div className="flex min-w-0 flex-col gap-2">
      {refs.map((entry) => (
        <div
          key={entry.label}
          role="group"
          aria-label={entry.label}
          className="flex min-w-0 flex-col gap-1"
        >
          <span className="text-xs font-medium text-[var(--color-text-soft)]">{entry.label}</span>
          <div className="flex min-w-0 flex-wrap gap-1">
            {entry.values.map((value) => (
              <Badge key={`${entry.label}:${value}`} variant={compact ? 'secondary' : undefined}>
                {value}
              </Badge>
            ))}
          </div>
        </div>
      ))}
    </div>
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
