import { useMemo, useState, type ReactNode } from 'react'
import { ArrowRight01Icon } from '@hugeicons/core-free-icons'
import { Link } from '@tanstack/react-router'

import { AppIcon } from '@/components/icons/app-icon'
import {
  AgentSessionDiagnostics,
  DiagnosticSection,
} from '@/components/observability/agent-session-diagnostics'
import {
  getAgentSessionToolMetricAvailability,
  type MetricAvailability,
} from '@/components/observability/agent-session-metrics'
import {
  Timeline,
  TimelineContent,
  TimelineDate,
  TimelineHeader,
  TimelineIndicator,
  TimelineItem,
  TimelineSeparator,
  TimelineTitle,
} from '@/components/observability/agent-sessions/timeline'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import type {
  AgentObservationView,
  AgentSessionDetailView,
  AgentSessionRequestView,
} from '@/types/api'

type AgentRequestAttempt = NonNullable<
  AgentSessionDetailView['report']
>['diagnostics']['reliability']['attempts'][number]

const currencyFormatters = {
  standard: new Intl.NumberFormat('en-GB', {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }),
  precise: new Intl.NumberFormat('en-GB', {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: 4,
    maximumFractionDigits: 4,
  }),
}

function formatCost(value?: number | null) {
  if (value === null || value === undefined) return '—'
  return currencyFormatters[value < 0.01 ? 'precise' : 'standard'].format(value)
}

function formatDuration(value?: number | null) {
  if (value === null || value === undefined) return '—'
  if (value < 1_000) return `${value} ms`
  if (value < 60_000) return `${(value / 1_000).toFixed(1)} s`
  return `${(value / 60_000).toFixed(1)} min`
}

export function SessionDetail({
  detail,
  showScore,
  canAccessRequestLogs,
}: {
  detail: AgentSessionDetailView
  showScore: boolean
  canAccessRequestLogs: boolean
}) {
  const report = detail.report
  const components = report?.components
  const diagnostics = report?.diagnostics
  return (
    <>
      <div>
        <section aria-label="Session summary" className="grid grid-cols-3 divide-x border-x px-2">
          <Metric
            label="Session score"
            value={showScore ? formatNullable(report?.score) : 'Score not shown'}
          />
          <Metric label="Normalised cost" value={formatCost(detail.session.normalized_cost_usd)} />
          <Metric label="Total time" value={formatDuration(components?.wall_time_ms)} />
        </section>

        {!showScore ? (
          <Alert>
            <AlertTitle>Calibration data</AlertTitle>
            <AlertDescription>
              The system does not show the session score during calibration. You can review session
              boundaries, outcomes, comparison groups, and data coverage.
            </AlertDescription>
          </Alert>
        ) : null}

        {detail.session.limitations.length > 0 ? (
          <Alert>
            <AlertTitle>Data limits</AlertTitle>
            <AlertDescription>
              {detail.session.limitations.map(formatLimitation).join(' · ')}
            </AlertDescription>
          </Alert>
        ) : null}

        {detail.request_history_truncated || detail.observation_history_truncated ? (
          <Alert>
            <AlertTitle>Some history is not shown</AlertTitle>
            <AlertDescription>
              This view shows a maximum of 1,000 requests and 1,000 detected activities. Open
              request logs to review retained request history.
            </AlertDescription>
          </Alert>
        ) : null}
      </div>
      <div className="divide-y border-y">
        <SessionEventStream
          requests={detail.requests}
          observations={detail.observations}
          historyTruncated={detail.request_history_truncated}
          attempts={diagnostics?.reliability.attempts ?? []}
          canAccessRequestLogs={canAccessRequestLogs}
        />
        <ToolExposure
          observations={detail.observations}
          availability={getAgentSessionToolMetricAvailability(detail)}
        />
        <AgentSessionDiagnostics
          detail={detail}
          formatCost={formatCost}
          formatDuration={formatDuration}
        />
      </div>
    </>
  )
}

const TOOL_PREVIEW_LIMIT = 6

interface ToolExposureItem {
  name: string
  totalTokens: number
  callCount: number
}

function SessionEventStream({
  requests,
  observations,
  attempts,
  historyTruncated,
  canAccessRequestLogs,
}: {
  requests: AgentSessionRequestView[]
  observations: AgentObservationView[]
  attempts: AgentRequestAttempt[]
  historyTruncated: boolean
  canAccessRequestLogs: boolean
}) {
  const [visibleCount, setVisibleCount] = useState(25)
  const visible = requests.slice(0, visibleCount)
  const observationsByRequest = useMemo(() => {
    const grouped = new Map<string, AgentObservationView[]>()
    for (const observation of observations) {
      const events = grouped.get(observation.source_request_id) ?? []
      events.push(observation)
      grouped.set(observation.source_request_id, events)
    }
    return grouped
  }, [observations])
  const attemptsByRequest = useMemo(() => {
    const grouped = new Map<string, AgentRequestAttempt[]>()
    for (const attempt of attempts) {
      const requestAttempts = grouped.get(attempt.request_id) ?? []
      requestAttempts.push(attempt)
      grouped.set(attempt.request_id, requestAttempts)
    }
    return grouped
  }, [attempts])

  return (
    <DiagnosticSection
      title="Event stream"
      summary={formatRequestCountSummary(requests.length, historyTruncated)}
      defaultOpen
    >
      {requests.length > 0 ? (
        <div className="space-y-4">
          <Timeline defaultValue={visible.length}>
            {visible.map((request, index) => {
              const requestObservations = observationsByRequest.get(request.request_id) ?? []
              const requestAttempts = attemptsByRequest.get(request.request_id) ?? []
              return (
                <TimelineItem key={request.request_id} step={index + 1}>
                  <TimelineIndicator
                    className={
                      request.terminal_success === false
                        ? 'border-destructive bg-destructive/15'
                        : 'bg-background'
                    }
                  />
                  <TimelineSeparator />
                  <RequestLogEntryLink
                    enabled={canAccessRequestLogs}
                    requestId={request.request_id}
                  >
                    <TimelineHeader className="flex items-start justify-between gap-3">
                      <div className="flex min-w-0 items-baseline gap-2">
                        <TimelineTitle className="shrink-0">
                          Request {request.ordinal + 1}
                        </TimelineTitle>
                        <p className="text-muted-foreground truncate font-mono text-xs">
                          {request.request_id}
                        </p>
                      </div>
                      <div className="flex shrink-0 items-center gap-2">
                        <TimelineDate
                          dateTime={request.occurred_at}
                          className="mb-0 font-normal tabular-nums"
                        >
                          {formatTimestamp(request.occurred_at)}
                        </TimelineDate>
                        {canAccessRequestLogs ? (
                          <AppIcon
                            icon={ArrowRight01Icon}
                            size={14}
                            stroke={1.5}
                            className="text-muted-foreground transition-transform group-hover/event:translate-x-0.5"
                            aria-hidden
                          />
                        ) : null}
                      </div>
                    </TimelineHeader>
                    <TimelineContent className="mt-2 flex flex-wrap gap-1.5">
                      <StateBadge
                        value={
                          request.terminal_success === true
                            ? 'succeeded'
                            : request.terminal_success === false
                              ? 'failed'
                              : 'unknown'
                        }
                      />
                      <Badge variant="outline">
                        {humanize(request.correlation_confidence)} confidence
                      </Badge>
                      <Badge variant="outline">{formatRequestDuration(request)}</Badge>
                      <Badge variant="outline">
                        {requestObservations.length}{' '}
                        {requestObservations.length === 1 ? 'activity' : 'activities'}
                      </Badge>
                      <Badge variant="outline">
                        {requestAttempts.length > 0
                          ? `${requestAttempts.length} ${requestAttempts.length === 1 ? 'attempt' : 'attempts'}`
                          : 'Attempts not measured'}
                      </Badge>
                      {requestAttempts.length > 0 ? (
                        <span className="text-muted-foreground basis-full text-xs">
                          {requestAttempts
                            .map(
                              (attempt) =>
                                `${attempt.provider_key}/${attempt.upstream_model}: ${humanize(attempt.status)}`,
                            )
                            .join(' → ')}
                        </span>
                      ) : null}
                      {requestObservations.length > 0 ? (
                        <span className="text-muted-foreground basis-full text-xs">
                          {requestObservations.map(formatObservationSummary).join(' · ')}
                        </span>
                      ) : null}
                    </TimelineContent>
                  </RequestLogEntryLink>
                </TimelineItem>
              )
            })}
          </Timeline>
          {visibleCount < requests.length ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => setVisibleCount((count) => Math.min(count + 25, requests.length))}
            >
              Show 25 more requests
            </Button>
          ) : null}
        </div>
      ) : (
        <p className="text-muted-foreground text-sm">No requests recorded.</p>
      )}
    </DiagnosticSection>
  )
}

function RequestLogEntryLink({
  enabled,
  requestId,
  children,
}: {
  enabled: boolean
  requestId: string
  children: ReactNode
}) {
  const className =
    'group/event block rounded-md px-2 py-2 focus-visible:ring-ring focus-visible:ring-2 focus-visible:outline-none'
  if (!enabled) {
    return <div className={className}>{children}</div>
  }
  return (
    <Link
      to="/observability/request-logs"
      search={{ request_id: requestId }}
      aria-label={`Open request ${requestId} in request logs`}
      className={`${className} hover:bg-muted/50 transition-colors`}
    >
      {children}
    </Link>
  )
}

function ToolExposure({
  observations,
  availability,
}: {
  observations: AgentObservationView[]
  availability: MetricAvailability
}) {
  const tools = useMemo(() => summarizeToolExposure(observations), [observations])
  const used = tools.filter((tool) => tool.callCount > 0)
  const neverCalled = tools.filter((tool) => tool.callCount === 0)

  return (
    <DiagnosticSection
      title="Tool exposure"
      summary={
        availability === 'measured'
          ? `${used.length} used · ${neverCalled.length} never called`
          : undefined
      }
      availability={availability}
    >
      <div className="grid gap-3 xl:grid-cols-2">
        <ToolExposurePanel
          title="Used at least once"
          description="Tools called during this session."
          tools={used}
          showCallCount
        />
        <ToolExposurePanel
          title="Never called"
          description="Supplied tools with no detected call."
          tools={neverCalled}
        />
      </div>
    </DiagnosticSection>
  )
}

function ToolExposurePanel({
  title,
  description,
  tools,
  showCallCount = false,
}: {
  title: string
  description: string
  tools: ToolExposureItem[]
  showCallCount?: boolean
}) {
  const [expanded, setExpanded] = useState(false)
  const visible = expanded ? tools : tools.slice(0, TOOL_PREVIEW_LIMIT)

  return (
    <section className="overflow-hidden rounded-lg border">
      <header className="flex items-start justify-between gap-3 px-3 py-3">
        <div>
          <h4 className="font-medium">{title}</h4>
          <p className="text-muted-foreground mt-0.5 text-xs">{description}</p>
        </div>
        <Badge variant="outline" className="tabular-nums">
          {tools.length}
        </Badge>
      </header>
      {visible.length > 0 ? (
        <ul className="divide-y border-t">
          {visible.map((tool) => (
            <li key={tool.name} className="flex items-center gap-3 px-3 py-2.5">
              <div className="min-w-0 flex-1">
                <p className="truncate font-mono text-xs">{tool.name}</p>
                {showCallCount ? (
                  <p className="text-muted-foreground mt-0.5 text-xs tabular-nums">
                    {tool.callCount} {tool.callCount === 1 ? 'call' : 'calls'}
                  </p>
                ) : null}
              </div>
              <Badge variant="outline" className="shrink-0 tabular-nums">
                {tool.totalTokens.toLocaleString()} tokens
              </Badge>
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-muted-foreground border-t px-3 py-4 text-sm">
          {showCallCount
            ? 'No tool calls were detected.'
            : 'No uncalled tools were present in retained tool-definition metadata.'}
        </p>
      )}
      {tools.length > TOOL_PREVIEW_LIMIT ? (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="w-full rounded-none border-t"
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? 'Show fewer' : `Show all ${tools.length}`}
        </Button>
      ) : null}
    </section>
  )
}

function summarizeToolExposure(observations: AgentObservationView[]): ToolExposureItem[] {
  const tools = new Map<string, ToolExposureItem & { hasDefinitionTokens: boolean }>()
  for (const observation of observations) {
    for (const supplied of observation.facts.supplied_tools ?? []) {
      const name = supplied.name.trim()
      if (!name) continue
      const tool = tools.get(name) ?? {
        name,
        totalTokens: 0,
        callCount: 0,
        hasDefinitionTokens: true,
      }
      tool.totalTokens += supplied.token_estimate
      tool.hasDefinitionTokens = true
      tools.set(name, tool)
    }
  }
  for (const observation of observations) {
    const name = observation.facts.tool_name?.trim()
    if (!name) continue
    const tool = tools.get(name) ?? {
      name,
      totalTokens: 0,
      callCount: 0,
      hasDefinitionTokens: false,
    }
    tool.callCount += 1
    if (!tool.hasDefinitionTokens) {
      tool.totalTokens += observation.facts.tool_schema_token_estimate ?? 0
    }
    tools.set(name, tool)
  }
  return [...tools.values()].sort(
    (left, right) => right.totalTokens - left.totalTokens || left.name.localeCompare(right.name),
  )
}

function formatObservationSummary(observation: AgentObservationView) {
  const file = observation.facts.file_interactions[0]
  if (file) {
    return `${humanize(file.operation)} ${file.opaque_file_id}${file.succeeded === false ? ' failed' : ''}`
  }
  const skill = observation.facts.supplied_skills.find((item) => item.used)
  if (skill) return `Used skill ${skill.name}`
  if (observation.facts.finish_reason) {
    return `Finished: ${humanize(observation.facts.finish_reason)}`
  }
  if (observation.facts.incomplete_reason) {
    return `Incomplete: ${humanize(observation.facts.incomplete_reason)}`
  }
  if (observation.facts.tool_name) return `Called ${observation.facts.tool_name}`
  return humanize(observation.kind)
}

function formatRequestDuration(request: AgentSessionRequestView) {
  if (!request.completed_at) return 'In progress'
  return formatDuration(
    Math.max(0, new Date(request.completed_at).getTime() - new Date(request.occurred_at).getTime()),
  )
}

function formatRequestCountSummary(count: number, truncated: boolean) {
  return `${count}${truncated ? '+' : ''} ${count === 1 ? 'request' : 'requests'}`
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 p-3">
      <p className="text-muted-foreground text-xs">{label}</p>
      <p className="mt-1 text-base leading-tight font-semibold tabular-nums sm:text-lg">{value}</p>
    </div>
  )
}

export function DetailSkeleton() {
  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-3">
        {Array.from({ length: 4 }).map((_, index) => (
          <Skeleton key={index} className="h-20" />
        ))}
      </div>
      <Skeleton className="h-36" />
      <Skeleton className="h-52" />
    </div>
  )
}

export function StateBadge({ value }: { value: string }) {
  const normalized = value.toLowerCase()
  const variant = normalized === 'failed' ? 'destructive' : 'outline'
  return <Badge variant={variant}>{humanize(value)}</Badge>
}

const timestampFormatter = new Intl.DateTimeFormat('en-GB', {
  dateStyle: 'medium',
  timeStyle: 'short',
  timeZone: 'UTC',
})

function formatTimestamp(value: string) {
  return timestampFormatter.format(new Date(value))
}

function formatNullable(value: unknown, fallback = '—') {
  return value === null || value === undefined ? fallback : String(value)
}

const limitationLabels: Record<string, string> = {
  cohort_fallback: 'The score uses a broader comparison group',
  late_data_excluded: 'Late data is not included',
  payload_truncated: 'Response data is incomplete',
  payload_unavailable: 'Response data is not available',
  pricing_unavailable: 'Pricing data is not available',
  request_incomplete: 'A request did not complete',
  semantic_verification_unavailable: 'Answer verification is not available',
  session_unobserved: 'An external session ID was not observed',
  tool_inventory_potential_only: 'The available tool list is estimated',
  usage_unavailable: 'Usage data is not available',
}

function formatLimitation(value: string) {
  return limitationLabels[value] ?? humanize(value)
}

function humanize(value?: string | null) {
  return value
    ? value.replaceAll('_', ' ').replace(/\b\w/g, (character) => character.toUpperCase())
    : 'Unknown'
}
