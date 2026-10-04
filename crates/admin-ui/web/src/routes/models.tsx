import { useEffect, useRef, useState, type ComponentProps, type ReactNode } from 'react'
import { createFileRoute, useLocation, useRouter, useRouterState } from '@tanstack/react-router'
import {
  BadgeInfoIcon,
  CodeIcon,
  ColumnsThreeCogIcon,
  Copy01Icon,
  HomeIcon,
  RefreshIcon,
  Tick02Icon,
} from '@hugeicons/core-free-icons'
import { toast } from 'sonner'

import { BrandIcon } from '@/components/icons/brand-icon'
import { AppIcon } from '@/components/icons/app-icon'
import { AgentHarnessLabel } from '@/components/icons/agent-harness-icon'
import {
  CodeBlock,
  CodeBlockCopyButton,
  CodeBlockHeader,
  CodeBlockTitle,
} from '@/components/reui/code-block/code-block'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { PageHeader } from '@/components/layout/page-header'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from '@/components/ui/empty'
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
import { isPlatformAdminSession } from '@/routes/-auth-routing'
import {
  BenchmarkAttribution,
  IntelligenceIndexLabel,
  ModelIntelligenceScore,
} from '@/routes/-model-benchmarks'
import {
  getModelClientConfigs,
  getModels,
  refreshModelPricing,
} from '@/server/admin-data.functions'
import type { ModelView } from '@/types/api'
import { ModelInfoDialog, type ModelInfoSectionKey } from '@/routes/-model-info-dialog'
import { ModelListPagination, ModelSearch } from '@/routes/-model-list-controls'
import { formatCost, formatWindow, providerTypeLabel } from '@/routes/-model-formatting'
import {
  CapabilityBadges,
  ModelAllowlistDetail,
  ModelStatusIndicator,
} from '@/routes/-model-presentation'

const DEFAULT_PAGE = 1
const DEFAULT_PAGE_SIZE = 30

const CLIENT_HARNESS_CONFIGURATION_URL =
  'https://oceans-llm.com/configuration/client-harness-configuration.html'

export const Route = createFileRoute('/models')({
  validateSearch: (search: Record<string, unknown>) => normalizeModelsSearch(search),
  loaderDeps: ({ search }) => search,
  loader: ({ deps }) => getModels({ data: { ...deps, include_aliases: false } }),
  component: ModelsPage,
})

// Selection, pagination, pricing refresh, and client configuration share one model catalog state.
// oxlint-disable-next-line eslint/max-lines-per-function
export function ModelsPage() {
  const { data: modelPage } = Route.useLoaderData()
  const { session } = Route.useRouteContext()
  const isPlatformAdmin = isPlatformAdminSession(session)
  const { query, updateQuery, isSearchPending } = useModelSearch()
  const isPagePending = useRouterState({ select: (state) => state.status === 'pending' })
  const router = useRouter()
  const [configDialog, setConfigDialog] = useState<{
    models: ModelView[]
    activeKey: string
    clientConfigurations: ModelView['client_configurations']
  } | null>(null)
  const [infoDialogModel, setInfoDialogModel] = useState<ModelView | null>(null)
  const infoDialogTrigger = useRef<HTMLElement | null>(null)
  const [modelInfoSection, setModelInfoSection] = useState<ModelInfoSectionKey>('overview')
  const [selectedModelsById, setSelectedModelsById] = useState<Record<string, ModelView>>({})
  const [visibleColumns, setVisibleColumns] = useState({
    contextWindow: false,
    capabilities: false,
  })
  const [isGeneratingConfig, setIsGeneratingConfig] = useState(false)
  const [isRefreshingPricing, setIsRefreshingPricing] = useState(false)
  const selectableModels = modelPage.items.filter((model) => model.client_configurations.length > 0)
  const selectedModels = Object.values(selectedModelsById)
  const selectedModelIds = Object.keys(selectedModelsById)
  const selectedModelIdSet = new Set(selectedModelIds)
  const allSelectableSelected =
    selectableModels.length > 0 &&
    selectableModels.every((model) => selectedModelIdSet.has(model.id))
  const desktopColumns = modelTableColumns(isPlatformAdmin, visibleColumns)
  const desktopColumnsWidth = desktopColumns.reduce((total, column) => total + column.width, 0)

  function navigateToPage(page: number, pageSize: number) {
    void router.navigate({
      to: '/models',
      search: (previous) =>
        normalizeModelsSearch({
          ...previous,
          page,
          page_size: pageSize,
        }),
      resetScroll: false,
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
    infoDialogTrigger.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null
    setModelInfoSection('overview')
    setInfoDialogModel(model)
  }

  const activeClientConfig =
    configDialog?.clientConfigurations.find((config) => config.key === configDialog.activeKey) ??
    configDialog?.clientConfigurations[0] ??
    null

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Control Plane"
        title="Models"
        description="Review the models that users can select and check their current status."
      />

      <Card className="min-w-0">
        <CardHeader>
          <CardTitle>Model list</CardTitle>
          <CardDescription>Select models to create a configuration file.</CardDescription>
        </CardHeader>
        <CardContent className="flex min-w-0 flex-col gap-4">
          <div className="flex min-w-0 flex-wrap items-center justify-between gap-3">
            <ModelSearch query={query} onQueryChange={updateQuery} />
            <div className="hidden flex-wrap items-center gap-3 md:flex">
              {selectedModelIds.length > 0 ? (
                <span className="text-subtle-foreground text-sm">
                  {selectedModelIds.length} selected for client config
                </span>
              ) : null}
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
                      <AppIcon
                        icon={ColumnsThreeCogIcon}
                        size={14}
                        stroke={1.5}
                        data-icon="inline-start"
                      />
                      Columns
                    </Button>
                  </PopoverTrigger>
                  <PopoverContent align="end" className="w-64 gap-3 p-3">
                    <div className="flex flex-col gap-1">
                      <h2 className="text-foreground text-sm font-medium">Table columns</h2>
                      <p className="text-subtle-foreground text-xs">
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
                          <span className="text-foreground font-medium">Context window</span>
                          <span className="text-subtle-foreground text-xs">
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
                          <span className="text-foreground font-medium">Capabilities</span>
                          <span className="text-subtle-foreground text-xs">
                            Streaming, vision, tools, and attachment support.
                          </span>
                        </span>
                      </label>
                    </div>
                  </PopoverContent>
                </Popover>
              </div>
              <div className="flex flex-wrap gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="gap-2"
                  onClick={openSelectedClientConfig}
                  disabled={selectedModelIds.length === 0 || isGeneratingConfig}
                >
                  <AppIcon icon={CodeIcon} size={14} stroke={1.5} data-icon="inline-start" />
                  Generate config
                </Button>
                {isPlatformAdmin ? (
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
                ) : null}
              </div>
            </div>
          </div>

          {modelPage.items.length === 0 ? (
            <Empty>
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <AppIcon icon={HomeIcon} size={22} stroke={1.5} />
                </EmptyMedia>
                <EmptyTitle>{query.trim() ? 'No models found' : 'No models configured'}</EmptyTitle>
                <EmptyDescription>
                  {query.trim()
                    ? 'Try another model ID, alias, provider, or tag.'
                    : 'Add at least one routed model before sending traffic through the gateway.'}
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <>
              <div className="grid gap-4 md:hidden" data-testid="models-mobile-list">
                {modelPage.items.map((model) => (
                  <ModelCard
                    key={model.id}
                    model={model}
                    showAccessDetails={isPlatformAdmin}
                    onCopy={(modelId) => handleCopyValue(modelId, 'Model ID copied')}
                    onOpenClientConfig={openSingleClientConfig}
                    onOpenInfo={openModelInfo}
                  />
                ))}
              </div>

              <div
                className="border-border hidden min-w-0 overflow-hidden rounded-md border md:block"
                data-testid="models-desktop-table"
              >
                <Table
                  className="table-fixed"
                  style={{ minWidth: `${desktopColumnsWidth + 3}rem` }}
                >
                  <colgroup>
                    <col className="w-12" />
                    {desktopColumns.map((column) => (
                      <col
                        key={column.key}
                        style={{
                          width: column.flexible ? undefined : `${column.width}rem`,
                        }}
                      />
                    ))}
                  </colgroup>
                  <ModelTableHeader
                    allSelected={allSelectableSelected}
                    hasSelectableModels={selectableModels.length > 0}
                    visibleColumns={visibleColumns}
                    showAccessDetails={isPlatformAdmin}
                    onToggleAll={toggleAllSelectableModels}
                  />
                  <TableBody>
                    {modelPage.items.map((model) => (
                      <TableRow key={model.id} className="group hover:bg-muted align-middle">
                        <TableCell className="bg-card group-hover:bg-muted sticky left-0 z-20 px-3 py-1 transition-colors">
                          <ModelCheckbox
                            aria-label={`Select model ${model.id}`}
                            checked={selectedModelIdSet.has(model.id)}
                            disabled={model.client_configurations.length === 0}
                            onChange={() => toggleModelSelection(model)}
                          />
                        </TableCell>
                        <TableCell
                          className="bg-card group-hover:bg-muted lg:shadow-sticky-edge z-20 px-3 py-1 transition-colors lg:sticky lg:left-[3rem]"
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
                                  <span className="text-foreground truncate font-semibold">
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
                              </div>
                            </div>
                          </div>
                        </TableCell>
                        <TableCell className="px-3 py-1 whitespace-normal">
                          <ModelActions
                            model={model}
                            onOpenClientConfig={openSingleClientConfig}
                            onOpenInfo={openModelInfo}
                          />
                        </TableCell>
                        <TableCell className="px-3 py-1">
                          <div className="flex min-w-0 flex-col gap-2 py-1">
                            <div className="flex min-w-0 items-center gap-2">
                              <BrandIcon iconKey={model.model_icon_key} size={14} />
                              <span className="text-foreground truncate">
                                {model.upstream_model ?? 'Not currently routed'}
                              </span>
                            </div>
                            <div className="tracking-label text-muted-foreground flex min-w-0 items-center gap-2 truncate text-xs">
                              <BrandIcon
                                iconKey={model.provider_icon_key}
                                size={14}
                                className="shrink-0"
                              />
                              <span className="truncate">{providerTypeLabel(model)}</span>
                            </div>
                          </div>
                        </TableCell>
                        <TableCell className="px-3 py-1 whitespace-normal tabular-nums">
                          <ModelIntelligenceScore model={model} />
                        </TableCell>
                        <TableCell className="px-3 py-1 whitespace-normal">
                          <StackedMetric
                            topLabel="Input"
                            topValue={formatCost(model.input_cost_per_million_tokens_usd_10000)}
                            bottomLabel="Output"
                            bottomValue={formatCost(model.output_cost_per_million_tokens_usd_10000)}
                          />
                        </TableCell>
                        {visibleColumns.contextWindow ? (
                          <TableCell className="px-3 py-1 whitespace-normal">
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
                          <TableCell className="px-3 py-1 whitespace-normal">
                            <CapabilityBadges model={model} />
                          </TableCell>
                        ) : null}
                        {isPlatformAdmin ? (
                          <TableCell className="px-3 py-1 whitespace-normal">
                            <ModelAllowlistDetail model={model} compact />
                          </TableCell>
                        ) : null}
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
            </>
          )}

          <ModelListPagination
            modelPage={modelPage}
            onPageChange={navigateToPage}
            isPending={isSearchPending || isPagePending}
          />
        </CardContent>
      </Card>
      <p className="text-muted-foreground text-right text-xs">
        <BenchmarkAttribution />
      </p>

      <ClientConfigDialog
        models={configDialog?.models ?? []}
        activeKey={configDialog?.activeKey ?? null}
        activeConfig={activeClientConfig}
        clientConfigurations={configDialog?.clientConfigurations ?? []}
        onActiveKeyChange={(activeKey) =>
          setConfigDialog((current) => (current ? { ...current, activeKey } : current))
        }
        onOpenChange={(open) => {
          if (!open) {
            setConfigDialog(null)
          }
        }}
      />
      <ModelInfoDialog
        model={infoDialogModel}
        activeSection={modelInfoSection}
        showAccessDetails={isPlatformAdmin}
        onCloseAutoFocus={(event) => {
          if (infoDialogTrigger.current?.isConnected) {
            event.preventDefault()
            infoDialogTrigger.current.focus()
          }
        }}
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

type VisibleModelColumns = {
  contextWindow: boolean
  capabilities: boolean
}

function modelTableColumns(isPlatformAdmin: boolean, visibleColumns: VisibleModelColumns) {
  // Names share the remaining space; controls and metrics keep compact widths.
  return [
    { key: 'model', width: 18, flexible: true },
    { key: 'actions', width: 11 },
    { key: 'provider', width: 20, flexible: true },
    { key: 'intelligence', width: 12 },
    { key: 'cost', width: 11 },
    ...(visibleColumns.contextWindow ? [{ key: 'context', width: 11 }] : []),
    ...(visibleColumns.capabilities ? [{ key: 'capabilities', width: 18 }] : []),
    ...(isPlatformAdmin ? [{ key: 'access', width: 10 }] : []),
  ]
}

function ModelTableHeader({
  allSelected,
  hasSelectableModels,
  visibleColumns,
  showAccessDetails,
  onToggleAll,
}: {
  allSelected: boolean
  hasSelectableModels: boolean
  visibleColumns: VisibleModelColumns
  showAccessDetails: boolean
  onToggleAll: () => void
}) {
  return (
    <TableHeader className="bg-surface-muted">
      <TableRow>
        <TableHead className="bg-surface-muted text-muted-foreground sticky left-0 z-30 px-3 py-2 font-semibold">
          <ModelCheckbox
            aria-label="Select all configurable models"
            checked={allSelected}
            disabled={!hasSelectableModels}
            onChange={onToggleAll}
          />
        </TableHead>
        <TableHead className="bg-surface-muted text-muted-foreground lg:shadow-sticky-edge z-30 px-3 py-2 font-semibold lg:sticky lg:left-[3rem]">
          Model ID
        </TableHead>
        <TableHead className="text-muted-foreground px-3 py-2 font-semibold">Actions</TableHead>
        <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
          Provider &amp; Model
        </TableHead>
        <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
          <IntelligenceIndexLabel />
        </TableHead>
        <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
          Cost / 1M tokens
        </TableHead>
        {visibleColumns.contextWindow ? (
          <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
            Context window
          </TableHead>
        ) : null}
        {visibleColumns.capabilities ? (
          <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
            Capabilities
          </TableHead>
        ) : null}
        {showAccessDetails ? (
          <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
            Allow List
          </TableHead>
        ) : null}
      </TableRow>
    </TableHeader>
  )
}

function ModelCard({
  model,
  onCopy,
  onOpenClientConfig,
  onOpenInfo,
  showAccessDetails,
}: {
  model: ModelView
  onCopy: (modelId: string) => void
  onOpenClientConfig: (model: ModelView) => void
  onOpenInfo: (model: ModelView) => void
  showAccessDetails: boolean
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
          <MetricDetail
            label={<IntelligenceIndexLabel />}
            value={<ModelIntelligenceScore model={model} />}
          />
          {showAccessDetails ? (
            <MetricDetail label="Model allowlist" value={<ModelAllowlistDetail model={model} />} />
          ) : null}
        </dl>
        <ModelNotes model={model} />
        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="gap-2"
            aria-label={`Model info for ${model.id}`}
            onClick={() => onOpenInfo(model)}
          >
            <AppIcon icon={BadgeInfoIcon} size={14} stroke={1.5} data-icon="inline-start" />
            Info
          </Button>
          <ClientConfigButton model={model} onOpen={onOpenClientConfig} />
        </div>
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
            variant="outline"
            size="sm"
            className="gap-2"
            onClick={() => onOpenInfo(model)}
          >
            <AppIcon icon={BadgeInfoIcon} size={14} stroke={1.5} data-icon="inline-start" />
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
        className="peer checked:border-primary checked:bg-primary focus-visible:border-ring focus-visible:ring-ring/50 border-border bg-surface-muted size-5 shrink-0 appearance-none rounded-md border transition-colors focus-visible:ring-3 disabled:cursor-not-allowed disabled:opacity-50"
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
    return <span className="text-muted-foreground">—</span>
  }

  const label = `Generate client config for ${model.id}`

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="gap-2"
          aria-label={compact ? label : undefined}
          onClick={() => onOpen(model)}
        >
          <AppIcon icon={CodeIcon} size={14} stroke={1.5} data-icon="inline-start" />
          {compact ? 'Config' : 'Client config'}
        </Button>
      </TooltipTrigger>
      <TooltipContent sideOffset={6}>{label}</TooltipContent>
    </Tooltip>
  )
}

// Keep the ordered setup, generated configuration blocks, and notes in one linear dialog.
// oxlint-disable-next-line eslint/max-lines-per-function
function ClientConfigDialog({
  models,
  activeKey,
  activeConfig,
  clientConfigurations,
  onActiveKeyChange,
  onOpenChange,
}: {
  models: ModelView[]
  activeKey: string | null
  activeConfig: ModelView['client_configurations'][number] | null
  clientConfigurations: ModelView['client_configurations']
  onActiveKeyChange: (key: string) => void
  onOpenChange: (open: boolean) => void
}) {
  const isOpen = models.length > 0
  const firstModel = models[0] ?? null
  const description =
    models.length === 1 && firstModel
      ? `${firstModel.id} via ${providerTypeLabel(firstModel)}`
      : `${models.length} selected models`

  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[min(880px,calc(100dvh-2rem))] max-w-[calc(100vw-2rem)] overflow-y-auto sm:max-w-[min(920px,calc(100vw-2rem))] md:min-w-[35vw]">
        <DialogHeader>
          <DialogTitle>Client config</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>

        {isOpen && activeConfig ? (
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
                spacing={1}
                className="max-w-full min-w-0 flex-wrap"
                aria-label="Client config"
              >
                {clientConfigurations.map((config) => (
                  <ToggleGroupItem key={config.key} value={config.key} aria-label={config.label}>
                    <AgentHarnessLabel className="px-2" harnessKey={config.key}>
                      {config.label}
                    </AgentHarnessLabel>
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
                  <TableRow>
                    <TableCell className="w-32 align-baseline font-medium whitespace-nowrap">
                      Base URL
                    </TableCell>
                    <TableCell className="text-muted-foreground min-w-0 align-baseline whitespace-normal">
                      Base URL can change depending on API format and client harness. Experiment
                      with adding or removing <code className="font-mono text-xs">/v1</code> if
                      requests initially fail. For more info, see:{' '}
                      <a
                        href={CLIENT_HARNESS_CONFIGURATION_URL}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="underline underline-offset-4"
                      >
                        client harness configuration
                      </a>
                      .
                    </TableCell>
                  </TableRow>
                </TableBody>
              </Table>
            ) : null}

            <div className="flex min-w-0 flex-col gap-4">
              {activeConfig.blocks.map((block) => (
                <CodeBlock
                  key={`${block.label}:${block.filename}`}
                  code={block.content}
                  language={configLanguage(block.filename)}
                  showLineNumbers
                  maxLines={activeConfig.key === 'claude-code' ? 10 : undefined}
                >
                  <CodeBlockHeader>
                    <CodeBlockTitle>{block.filename}</CodeBlockTitle>
                    {block.label !== block.filename ? (
                      <span className="text-muted-foreground min-w-0 truncate text-xs">
                        {block.label}
                      </span>
                    ) : null}
                    <CodeBlockCopyButton
                      className="ml-auto"
                      labels={{
                        copy: copyConfigLabel(block.filename),
                        copied: 'Copied',
                        failed: 'Copy failed',
                      }}
                      onCopy={() => toast.success('Client config copied')}
                      onCopyError={() => toast.error('Clipboard access failed')}
                    />
                  </CodeBlockHeader>
                </CodeBlock>
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

function configLanguage(filename: string) {
  if (filename.endsWith('.json')) {
    return 'json'
  }
  if (filename.endsWith('.toml')) {
    return 'toml'
  }
  return 'text'
}

function MetricDetail({
  label,
  mono = false,
  value,
}: {
  label: ReactNode
  mono?: boolean
  value: ReactNode
}) {
  return (
    <div>
      <dt className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
        {label}
      </dt>
      <dd className={mono ? 'text-subtle-foreground font-mono text-xs' : 'text-subtle-foreground'}>
        {value}
      </dd>
    </div>
  )
}

function ModelNotes({ model }: { model: ModelView }) {
  if (!model.description && model.tags.length === 0) {
    return <span className="text-muted-foreground">—</span>
  }

  return (
    <div className="flex min-w-0 flex-col gap-2 py-1">
      {model.description ? (
        <p className="text-subtle-foreground line-clamp-2 whitespace-normal">{model.description}</p>
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
    <div className="grid min-w-0 grid-cols-[auto_auto] items-center justify-start gap-x-4 gap-y-1 py-1">
      <span className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
        {topLabel}
      </span>
      <span className="text-subtle-foreground text-right tabular-nums">{topValue}</span>
      <span className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
        {bottomLabel}
      </span>
      <span className="text-subtle-foreground text-right tabular-nums">{bottomValue}</span>
    </div>
  )
}

function useModelSearch() {
  const router = useRouter()
  const location = useLocation()
  const urlQuery = typeof location.search.q === 'string' ? location.search.q : ''
  const [query, setQuery] = useState(urlQuery)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const currentLocation = useRef(location)
  const modelsPathname = useRef(location.pathname)
  const isOnModels = useRef(true)

  useEffect(() => {
    const unsubscribe = router.subscribe('onBeforeNavigate', ({ toLocation }) => {
      // Pricing refresh reloads the current location and must preserve any draft search.
      if (
        toLocation.href === currentLocation.current.href &&
        toLocation.state.__TSR_key === currentLocation.current.state.__TSR_key
      ) {
        return
      }
      currentLocation.current = toLocation
      // Cancel before the next loader starts, including Back, Forward, and leaving Models.
      clearTimeout(timer.current ?? undefined)
      timer.current = null
      isOnModels.current = toLocation.pathname === modelsPathname.current
      setQuery(normalizeModelsSearch(toLocation.search).q ?? '')
    })
    return () => {
      unsubscribe()
      clearTimeout(timer.current ?? undefined)
    }
  }, [router])

  function updateQuery(q: string) {
    setQuery(q)
    clearTimeout(timer.current ?? undefined)
    timer.current = null
    if (q === urlQuery || !isOnModels.current) return

    timer.current = setTimeout(() => {
      timer.current = null
      void router.navigate({
        to: '/models',
        search: (previous) => normalizeModelsSearch({ ...previous, q, page: 1 }),
        replace: true,
        resetScroll: false,
      })
    }, 250)
  }

  return { query, updateQuery, isSearchPending: query !== urlQuery }
}

function normalizeModelsSearch(search: Record<string, unknown>) {
  const page = Number(search.page)
  const pageSize = Number(search.page_size)

  return {
    page: Number.isFinite(page) && page >= 1 ? Math.floor(page) : DEFAULT_PAGE,
    page_size:
      Number.isFinite(pageSize) && pageSize >= 1
        ? Math.min(100, Math.floor(pageSize))
        : DEFAULT_PAGE_SIZE,
    q: typeof search.q === 'string' && search.q.length > 0 ? search.q : undefined,
  }
}
