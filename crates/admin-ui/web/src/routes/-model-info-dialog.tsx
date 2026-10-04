import { useId, type ComponentProps, type ReactNode } from 'react'
import { ArrowDown01Icon, Copy01Icon } from '@hugeicons/core-free-icons'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
import { BrandIcon } from '@/components/icons/brand-icon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { ModelBenchmarks } from '@/routes/-model-benchmarks'
import {
  CapabilityBadges,
  ModelAllowlistDetail,
  formatCost,
  formatWindow,
  providerTypeLabel,
} from '@/routes/-model-presentation'
import type { ModelView } from '@/types/api'

export type ModelInfoSectionKey = 'overview' | 'routing' | 'benchmarks' | 'access'
type ModelRouteView = NonNullable<ModelView['routes']>[number]

const SECTIONS = [
  {
    key: 'overview',
    label: 'Overview',
    description: 'Model capabilities, pricing, and context limits.',
  },
  {
    key: 'routing',
    label: 'Routing',
    description: 'Provider connections and how requests are assigned.',
  },
  {
    key: 'benchmarks',
    label: 'Benchmarks',
    description: 'Capability scores with their source and update date.',
  },
  {
    key: 'access',
    label: 'Access',
    description: 'Who can use this model.',
  },
] as const

export function ModelInfoDialog({
  model,
  activeSection,
  onActiveSectionChange,
  onOpenChange,
  onCloseAutoFocus,
  showAccessDetails,
}: {
  model: ModelView | null
  activeSection: ModelInfoSectionKey
  onActiveSectionChange: (section: ModelInfoSectionKey) => void
  onOpenChange: (open: boolean) => void
  onCloseAutoFocus?: ComponentProps<typeof DialogContent>['onCloseAutoFocus']
  showAccessDetails: boolean
}) {
  const panelId = useId()
  const sections = SECTIONS.filter((section) => showAccessDetails || section.key !== 'access')
  const active = sections.find((section) => section.key === activeSection) ?? SECTIONS[0]

  return (
    <Dialog open={model !== null} onOpenChange={onOpenChange}>
      <DialogContent
        onCloseAutoFocus={onCloseAutoFocus}
        className="flex max-h-[calc(100dvh-2rem)] min-w-0 flex-col gap-0 overflow-hidden p-0 sm:max-w-3xl"
      >
        {model ? (
          <>
            <DialogHeader className="min-w-0 gap-2 px-5 py-5 pr-12">
              <DialogTitle>Model info</DialogTitle>
              <DialogDescription className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-2">
                <span className="flex min-w-0 items-center gap-2">
                  <BrandIcon iconKey={model.model_icon_key} size={18} />
                  <span className="min-w-0 font-mono text-xs break-all">{model.id}</span>
                </span>
              </DialogDescription>
            </DialogHeader>

            <div
              data-testid="model-info-layout"
              className="flex min-h-0 min-w-0 flex-col overflow-hidden border-t md:flex-row"
            >
              <nav
                aria-label="Model info sections"
                className="flex shrink-0 gap-1 overflow-x-auto border-b p-2 md:w-36 md:flex-col md:border-r md:border-b-0 md:p-3"
              >
                {sections.map((section) => (
                  <Button
                    key={section.key}
                    type="button"
                    variant={active.key === section.key ? 'secondary' : 'ghost'}
                    size="sm"
                    className="shrink-0 justify-start"
                    aria-current={active.key === section.key ? 'page' : undefined}
                    aria-controls={panelId}
                    onClick={() => onActiveSectionChange(section.key)}
                  >
                    {section.label}
                  </Button>
                ))}
              </nav>

              <section
                id={panelId}
                aria-labelledby={`${panelId}-title`}
                data-testid="model-info-content"
                className="flex min-h-0 min-w-0 flex-1 flex-col gap-5 overflow-y-auto p-5"
              >
                <div className="flex flex-col gap-1">
                  <h3 id={`${panelId}-title`} className="text-sm font-medium">
                    {active.label}
                  </h3>
                  <p className="text-muted-foreground text-xs leading-relaxed">
                    {active.description}
                  </p>
                </div>
                {active.key === 'overview' ? <ModelInfoOverview model={model} /> : null}
                {active.key === 'routing' ? <ModelInfoRouting model={model} /> : null}
                {active.key === 'benchmarks' ? <ModelBenchmarks model={model} /> : null}
                {active.key === 'access' ? <ModelInfoAccess model={model} /> : null}
              </section>
            </div>
          </>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

function ModelInfoOverview({ model }: { model: ModelView }) {
  async function copyModelId() {
    try {
      await navigator.clipboard.writeText(model.id)
      toast.success('Model ID copied')
    } catch {
      toast.error('Clipboard access failed')
    }
  }

  return (
    <>
      <dl className="min-w-0">
        <ModelInfoRow
          label="Gateway model"
          value={
            <div className="flex min-w-0 items-center gap-2">
              <span className="min-w-0 font-mono text-xs break-all">{model.id}</span>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-xs"
                    aria-label="Copy model ID"
                    onClick={() => void copyModelId()}
                  >
                    <AppIcon icon={Copy01Icon} />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>Copy model ID</TooltipContent>
              </Tooltip>
            </div>
          }
        />
        <ModelInfoRow
          label="Aliases"
          value={
            model.aliases?.length ? (
              <ul className="flex min-w-0 flex-col gap-1.5 font-mono text-xs break-all">
                {model.aliases.map((alias) => (
                  <li key={alias}>{alias}</li>
                ))}
              </ul>
            ) : (
              'No aliases'
            )
          }
        />
        <ModelInfoRow
          label="Capabilities"
          value={<CapabilityBadges model={model} />}
          className="pt-5"
        />
        <ModelInfoRow
          label="Tags"
          value={
            model.tags.length > 0 ? (
              <div className="flex flex-wrap gap-1.5">
                {model.tags.map((tag) => (
                  <Badge key={tag} variant="outline">
                    {tag}
                  </Badge>
                ))}
              </div>
            ) : (
              'No tags'
            )
          }
        />
      </dl>
      <ModelInfoEconomics model={model} />
    </>
  )
}

function ModelInfoRouting({ model }: { model: ModelView }) {
  return (
    <div className="flex min-w-0 flex-col gap-5">
      {model.routes ? (
        <>
          <RoutingPolicy model={model} />
          {model.routes.length > 0 ? (
            <div className="min-w-0 divide-y border-y" aria-label="Provider routes">
              {model.routes.map((route, index) => (
                <ProviderRoute key={route.id} route={route} position={index + 1} />
              ))}
            </div>
          ) : (
            <Empty>
              <EmptyHeader>
                <EmptyTitle>No provider routes</EmptyTitle>
                <EmptyDescription>
                  This model has no configured provider connections.
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          )}
        </>
      ) : (
        <dl className="divide-y">
          <ModelInfoRow label="Provider" value={providerTypeLabel(model)} />
          <ModelInfoRow
            label="Upstream model"
            value={model.upstream_model ?? 'Not currently routed'}
            mono
          />
        </dl>
      )}
      <Collapsible className="min-w-0">
        <CollapsibleTrigger asChild>
          <Button type="button" variant="ghost" size="sm" className="group gap-2 px-0">
            <AppIcon
              icon={ArrowDown01Icon}
              data-icon="inline-start"
              className="transition-transform group-data-[state=open]:rotate-180"
            />
            Technical details
          </Button>
        </CollapsibleTrigger>
        <CollapsibleContent>
          <dl className="min-w-0 divide-y pt-2">
            <ModelInfoRow label="Model ID" value={model.model_id ?? '—'} mono />
            <ModelInfoRow label="Resolved model" value={model.resolved_model_key} mono />
            {!model.routes ? (
              <ModelInfoRow label="Provider key" value={model.provider_key ?? '—'} mono />
            ) : null}
          </dl>
        </CollapsibleContent>
      </Collapsible>
    </div>
  )
}

function RoutingPolicy({ model }: { model: ModelView }) {
  const strategy = model.routing?.strategy ?? 'weighted_random'
  const affinity = model.routing?.affinity
  const policies = {
    preferred: {
      label: 'Preferred order',
      description: 'New placements use the first eligible route at the lowest priority number.',
    },
    round_robin: {
      label: 'Round robin',
      description:
        'New placements take turns across eligible routes at the lowest priority number.',
    },
    weighted_random: {
      label: 'Weighted random',
      description: 'Weights control selection among eligible routes at the lowest priority number.',
    },
  }
  const policy = policies[strategy]

  return (
    <div className="flex min-w-0 flex-col gap-3">
      <dl className="grid min-w-0 grid-cols-2 gap-4">
        <div className="flex min-w-0 flex-col gap-1">
          <dt className="text-muted-foreground text-xs">Selection policy</dt>
          <dd className="text-sm font-medium">{policy.label}</dd>
        </div>
        <div className="flex min-w-0 flex-col gap-1 border-l pl-4">
          <dt className="text-muted-foreground text-xs">Session affinity</dt>
          <dd className="text-sm font-medium">
            {affinity ? `${formatIdleTimeout(affinity.idle_timeout_seconds)} idle` : 'Off'}
          </dd>
        </div>
      </dl>
      <p className="text-muted-foreground text-xs leading-relaxed">
        {policy.description}{' '}
        {affinity
          ? 'Active sessions keep their route. Successful requests refresh the idle timeout.'
          : 'Each request selects a route.'}
      </p>
    </div>
  )
}

function ProviderRoute({ route, position }: { route: ModelRouteView; position: number }) {
  const summaryId = useId()
  const label = route.provider_label ?? route.provider_key
  const state = !route.enabled
    ? 'Disabled'
    : !route.provider_configured
      ? 'Missing provider'
      : route.weight <= 0
        ? 'Zero weight'
        : 'Enabled'

  return (
    <Collapsible data-testid={`model-route-${route.id}`} className="min-w-0">
      <CollapsibleTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          aria-label={`Details for ${label}, route ${position}`}
          aria-describedby={summaryId}
          className="group h-auto w-full min-w-0 justify-start gap-3 rounded-none px-1 py-4 text-left whitespace-normal"
        >
          <BrandIcon iconKey={route.provider_icon_key} size={22} />
          <span id={summaryId} className="flex min-w-0 flex-1 flex-col gap-1.5">
            <span className="flex min-w-0 flex-wrap items-center gap-2">
              <span className="break-words">{label}</span>
              <Badge variant={state === 'Enabled' ? 'secondary' : 'outline'}>{state}</Badge>
            </span>
            <span className="text-muted-foreground min-w-0 font-mono text-xs break-all">
              {route.upstream_model}
            </span>
            <span className="text-muted-foreground text-xs">
              Priority {route.priority} <span aria-hidden>·</span> Weight {route.weight}
            </span>
          </span>
          <AppIcon
            icon={ArrowDown01Icon}
            data-icon="inline-end"
            className="shrink-0 transition-transform group-data-[state=open]:rotate-180"
          />
        </Button>
      </CollapsibleTrigger>
      <CollapsibleContent>
        <dl className="min-w-0 divide-y border-t pb-3 pl-9">
          <ModelInfoRow label="Provider key" value={route.provider_key} mono />
          <ModelInfoRow label="Route ID" value={route.id} mono />
        </dl>
      </CollapsibleContent>
    </Collapsible>
  )
}

function ModelInfoEconomics({ model }: { model: ModelView }) {
  return (
    <div className="flex min-w-0 flex-col gap-3 border-t pt-5">
      <div className="flex min-w-0 flex-col gap-1">
        <h4 className="text-sm font-medium">Pricing and limits</h4>
        <p className="text-muted-foreground text-xs leading-relaxed break-words">
          Prices in USD per 1 million tokens.
          <br />
          {pricingSourceLabel(model)}
          {model.upstream_model ? (
            <>
              {' '}
              Rates shown for {providerTypeLabel(model)} ({model.upstream_model}).
            </>
          ) : null}
          {model.pricing_varies_by_route || (model.routes?.length ?? 0) > 1 ? (
            <> Other routes may differ. Context limits reflect the eligible route pool.</>
          ) : null}
        </p>
      </div>
      <dl
        className="grid min-w-0 grid-cols-2 gap-x-6 gap-y-6 sm:gap-x-16"
        data-testid="model-pricing-costs"
      >
        <ModelPricingMetric
          label="Input cost"
          subtitle="per 1 million tokens"
          value={formatCost(model.input_cost_per_million_tokens_usd_10000)}
        />
        <ModelPricingMetric
          label="Output cost"
          subtitle="per 1 million tokens"
          value={formatCost(model.output_cost_per_million_tokens_usd_10000)}
        />
        <ModelPricingMetric
          label="Cache read"
          subtitle="per 1 million tokens"
          value={formatCost(model.cache_read_cost_per_million_tokens_usd_10000)}
        />
        <ModelPricingMetric
          label="Cache write"
          subtitle="per 1 million tokens"
          value={formatCost(model.cache_write_cost_per_million_tokens_usd_10000)}
        />
      </dl>
      <dl
        className="grid min-w-0 grid-cols-2 gap-x-6 gap-y-3 pt-5 sm:gap-x-16"
        data-testid="model-pricing-limits"
      >
        <ModelPricingMetric
          label="Input context window"
          value={formatWindow(model.input_window_tokens ?? model.context_window_tokens)}
        />
        <ModelPricingMetric
          label="Output context window"
          value={formatWindow(model.output_window_tokens)}
        />
        {model.input_window_tokens != null &&
        model.context_window_tokens != null &&
        model.input_window_tokens !== model.context_window_tokens ? (
          <ModelPricingMetric
            label="Total context window"
            value={formatWindow(model.context_window_tokens)}
          />
        ) : null}
      </dl>
    </div>
  )
}

function pricingSourceLabel(model: ModelView) {
  const source = model.pricing_source
  switch (source?.kind) {
    case 'configured_override':
      return 'Source: configured pricing.'
    case 'mixed':
      return 'Source: mixed pricing sources.'
    case 'catalog':
      if (source.catalog_source === 'models_dev_api') return 'Source: models.dev catalog.'
      if (source.catalog_source === 'vendored_models_dev') {
        return 'Source: bundled models.dev catalog.'
      }
      return source.catalog_source
        ? `Source: ${source.catalog_source}.`
        : 'Source: pricing catalog.'
    default:
      return 'Pricing source unavailable.'
  }
}

function ModelPricingMetric({
  label,
  subtitle,
  value,
}: {
  label: string
  subtitle?: string
  value: string
}) {
  return (
    <div className="grid min-w-0 gap-2 text-sm sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
      <dt className="text-muted-foreground min-w-0">
        {label}
        {subtitle ? <span className="mt-1 block text-xs">{subtitle}</span> : null}
      </dt>
      <dd className="min-w-0 break-words tabular-nums">{value}</dd>
    </div>
  )
}

function ModelInfoAccess({ model }: { model: ModelView }) {
  return (
    <dl className="min-w-0 divide-y">
      <ModelInfoRow label="Model allowlist" value={<ModelAllowlistDetail model={model} />} />
    </dl>
  )
}

function ModelInfoRow({
  label,
  mono = false,
  value,
  className,
}: {
  label: string
  mono?: boolean
  value: ReactNode
  className?: string
}) {
  return (
    <div
      className={cn('grid min-w-0 gap-2 py-3 text-sm sm:grid-cols-[9rem_minmax(0,1fr)]', className)}
    >
      <dt className="text-muted-foreground">{label}</dt>
      <dd className={cn('max-w-full min-w-0 break-words', mono && 'font-mono text-xs break-all')}>
        {value}
      </dd>
    </div>
  )
}

function formatIdleTimeout(seconds: number) {
  if (seconds % 3600 === 0) return `${seconds / 3600} ${seconds === 3600 ? 'hour' : 'hours'}`
  if (seconds % 60 === 0) return `${seconds / 60} ${seconds === 60 ? 'minute' : 'minutes'}`
  return `${seconds} ${seconds === 1 ? 'second' : 'seconds'}`
}
