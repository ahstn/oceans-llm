import { useEffect, useState, type ReactNode } from 'react'
import { ArrowRight01Icon, Copy01Icon, Tick02Icon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { BrandIcon } from '@/components/icons/brand-icon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { EMPTY_VALUE, formatCount, formatDuration } from '@/lib/format'
import { cn } from '@/lib/utils'
import type { RequestLogDetailView, RequestLogView } from '@/types/api'

import {
  cacheHitLabel,
  cacheHitPercent,
  callerLabel,
  formatOccurredAt,
  formatRequestCost,
  metadataBoolean,
  operationLabel,
  requestTagLabels,
  statusCodeBadgeVariant,
} from './format'

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

export function StatusBadge({ statusCode }: { statusCode: number | null | undefined }) {
  return (
    <Badge variant={statusCodeBadgeVariant(statusCode)} className="tabular-nums">
      {statusCode ?? 'No status'}
    </Badge>
  )
}

/**
 * A machine identifier on one line: it truncates instead of breaking mid-token, shows the full
 * value on hover and copies it with one click. `lg` is the sheet title.
 */
export function Identifier({
  value,
  label,
  size = 'sm',
}: {
  value: string
  label: string
  size?: 'sm' | 'lg'
}) {
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    if (!copied) return
    const timer = window.setTimeout(() => setCopied(false), 1500)
    return () => window.clearTimeout(timer)
  }, [copied])

  return (
    <span className="flex min-w-0 items-center gap-1">
      <span
        className={cn(
          'text-foreground min-w-0 truncate font-mono',
          size === 'lg' ? 'text-lg leading-tight font-medium' : 'text-sm',
        )}
        title={value}
      >
        {value}
      </span>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            className="text-muted-foreground hover:text-foreground shrink-0"
            aria-label={copied ? `${label} copied` : `Copy ${label}`}
            onClick={() => {
              void navigator.clipboard?.writeText(value).then(() => setCopied(true))
            }}
          >
            <AppIcon icon={copied ? Tick02Icon : Copy01Icon} size={14} stroke={1.5} aria-hidden />
          </Button>
        </TooltipTrigger>
        <TooltipContent>{copied ? 'Copied' : `Copy ${label}`}</TooltipContent>
      </Tooltip>
    </span>
  )
}

/** Requested model, the model it resolved to when different, and the provider that served it. */
export function ModelRoute({ log }: { log: RequestLogView }) {
  const resolvedDiffers = log.resolved_model_key && log.resolved_model_key !== log.model_key

  return (
    <span
      data-testid="request-log-model-route"
      className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-sm"
    >
      <span className="text-foreground inline-flex min-w-0 items-center gap-1.5 font-medium">
        <BrandIcon iconKey={log.model_icon_key} size={16} />
        <span className="truncate">{log.model_key}</span>
      </span>
      {resolvedDiffers ? (
        <span className="text-subtle-foreground inline-flex min-w-0 items-center gap-1">
          <AppIcon icon={ArrowRight01Icon} size={14} stroke={1.5} aria-label="resolved to" />
          <span className="truncate">{log.resolved_model_key}</span>
        </span>
      ) : null}
      <span className="text-muted-foreground inline-flex min-w-0 items-center gap-1.5">
        <span>via</span>
        <BrandIcon iconKey={log.provider_icon_key} size={14} />
        <span className="text-subtle-foreground truncate">{log.provider_key}</span>
      </span>
    </span>
  )
}

// ---------------------------------------------------------------------------
// Key metrics
// ---------------------------------------------------------------------------

interface Metric {
  label: string
  value: string
  hint?: string
}

/** The four facts an operator checks first, in a fixed order. */
function keyMetrics(log: RequestLogView): Metric[] {
  const cachePercent = cacheHitPercent(log)
  const cached = log.cache_read_tokens ?? 0

  return [
    { label: 'Latency', value: formatDuration(log.latency_ms) },
    {
      label: 'Tokens',
      value: formatCount(log.total_tokens),
      hint:
        log.prompt_tokens != null || log.completion_tokens != null
          ? `${formatCount(log.prompt_tokens)} in · ${formatCount(log.completion_tokens)} out`
          : undefined,
    },
    { label: 'Cost', value: formatRequestCost(log.cost_usd_10000) },
    {
      label: 'Cache hit',
      value: cachePercent != null ? `${cachePercent}%` : cached > 0 ? formatCount(cached) : 'None',
      hint:
        cached > 0
          ? log.prompt_tokens
            ? `${formatCount(cached)} of ${formatCount(log.prompt_tokens)} prompt tokens`
            : cacheHitLabel(cached, log.prompt_tokens)
          : 'No prompt tokens cached',
    },
  ]
}

/**
 * Latency, tokens, cost and cache hit as tiles split by hairlines. The `gap-px` grid over the
 * border colour keeps the lines right however many tiles wrap onto a row.
 */
export function KeyMetrics({ log }: { log: RequestLogView }) {
  return (
    <dl
      data-testid="request-log-key-metrics"
      className="bg-border ring-foreground/10 grid shrink-0 grid-cols-2 gap-px overflow-hidden rounded-xl ring-1 lg:grid-cols-4"
    >
      {keyMetrics(log).map((metric) => (
        <div key={metric.label} className="bg-card min-w-0 p-4">
          <dt className="text-muted-foreground text-xs">{metric.label}</dt>
          <dd className="text-foreground mt-1 truncate text-xl leading-tight font-semibold tabular-nums">
            {metric.value}
          </dd>
          {metric.hint ? (
            <dd
              className="text-muted-foreground mt-1 truncate text-xs tabular-nums"
              title={metric.hint}
            >
              {metric.hint}
            </dd>
          ) : null}
        </div>
      ))}
    </dl>
  )
}

// ---------------------------------------------------------------------------
// Facts
// ---------------------------------------------------------------------------

interface Fact {
  label: string
  value: ReactNode
}

/**
 * The secondary facts in two columns: who sent the request and how to find it again, then how
 * it ran and how the caller tagged it.
 */
export function RequestFacts({ detail }: { detail: RequestLogDetailView }) {
  const log = detail.log
  const operation = operationLabel(log)

  const caller: Fact[] = [
    { label: 'Caller', value: callerLabel(log) },
    {
      label: 'API key',
      value: log.api_key_name ?? <Identifier value={log.api_key_id} label="API key ID" />,
    },
    { label: 'Agent harness', value: log.agent_harness_label },
    {
      label: 'User-agent',
      value: detail.user_agent_raw ? (
        <span className="block truncate font-mono text-sm" title={detail.user_agent_raw}>
          {detail.user_agent_raw}
        </span>
      ) : (
        EMPTY_VALUE
      ),
    },
    { label: 'Request ID', value: <Identifier value={log.request_id} label="request ID" /> },
    {
      label: 'Request log ID',
      value: <Identifier value={log.request_log_id} label="request log ID" />,
    },
  ]
  const execution: Fact[] = [
    ...(operation ? [{ label: 'Operation', value: operation }] : []),
    { label: 'Stream', value: metadataBoolean(log, 'stream') ? 'Yes' : 'No' },
    {
      label: 'Occurred at',
      value: <span className="whitespace-nowrap">{formatOccurredAt(log.occurred_at)}</span>,
    },
    {
      label: 'Error code',
      value: log.error_code ? (
        <span className="text-destructive font-mono">{log.error_code}</span>
      ) : (
        EMPTY_VALUE
      ),
    },
    { label: 'Tags', value: <TagList log={log} /> },
  ]

  return (
    <section
      aria-label="Request facts"
      data-testid="request-log-facts"
      className="grid min-w-0 gap-x-10 lg:grid-cols-2"
    >
      <FactList facts={caller} />
      <FactList facts={execution} className="border-border border-t lg:border-t-0" />
    </section>
  )
}

/** Label/value rows divided by single hairlines, the label in a fixed column beside its value. */
function FactList({ facts, className }: { facts: Fact[]; className?: string }) {
  return (
    <dl className={cn('divide-border min-w-0 divide-y', className)}>
      {facts.map((fact) => (
        <div
          key={fact.label}
          className="grid min-w-0 grid-cols-[8.5rem_minmax(0,1fr)] items-center gap-3 py-2"
        >
          <dt className="text-muted-foreground text-xs">{fact.label}</dt>
          <dd className="text-foreground min-w-0 truncate text-sm">{fact.value}</dd>
        </div>
      ))}
    </dl>
  )
}

function TagList({ log }: { log: RequestLogView }) {
  const tags = requestTagLabels(log)

  if (tags.length === 0) {
    return <span className="text-muted-foreground text-sm">No caller tags</span>
  }

  return (
    <div className="flex min-w-0 flex-wrap gap-1.5 py-0.5">
      {tags.map((tag) => (
        <Badge key={tag} variant="outline" className="max-w-full font-mono">
          <span className="truncate">{tag}</span>
        </Badge>
      ))}
    </div>
  )
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

/** A section of the detail body: a hairline above, a heading, an optional action on the right. */
export function DetailSection({
  title,
  description,
  action,
  testId,
  children,
}: {
  title: string
  description: string
  action?: ReactNode
  testId?: string
  children: ReactNode
}) {
  return (
    <section
      data-testid={testId}
      className="border-border flex min-w-0 flex-col gap-4 border-t pt-5"
    >
      <div className="flex min-w-0 flex-wrap items-end justify-between gap-x-4 gap-y-2">
        <div className="min-w-0">
          <h3 className="type-headline text-foreground">{title}</h3>
          <p className="text-muted-foreground text-sm">{description}</p>
        </div>
        {action}
      </div>
      {children}
    </section>
  )
}
