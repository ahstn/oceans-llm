import { useState } from 'react'
import { Add01Icon, Alert02Icon, ArrowRight01Icon, RefreshIcon } from '@hugeicons/core-free-icons'
import { AppIcon } from '@/components/icons/app-icon'
import {
  Frame,
  FrameDescription,
  FrameFooter,
  FrameHeader,
  FramePanel,
  FrameTitle,
} from '@/components/reui/frame'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import { cn } from '@/lib/utils'
import type { McpServerView } from '@/types/api'
import {
  authLabel,
  discoveredAt,
  discoveryLabel,
  endpointHost,
  needsAttention,
  type CandidateProps,
} from './model'
import {
  CandidateToolbar,
  DiscoveryBadge,
  NoServers,
  RegistrationBadge,
  ServerMark,
} from './shared'

export function OperationsCandidate(props: CandidateProps) {
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const orderedServers = [...props.servers].sort(
    (left, right) => Number(needsAttention(right)) - Number(needsAttention(left)),
  )
  const selected = orderedServers.find((server) => server.id === selectedId) ?? orderedServers[0]
  const attentionCount = props.allServers.filter(needsAttention).length

  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <div className="flex flex-col gap-2">
          <p className="text-muted-foreground text-xs font-medium tracking-widest uppercase">
            Discovery workspace
          </p>
          <h1 className="text-2xl font-semibold tracking-tight">MCP servers</h1>
          <p className="text-muted-foreground text-sm">
            Review discovery results and keep your tool connections up to date.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button type="button" variant="outline" onClick={props.onCatalog}>
            Browse catalog
          </Button>
          <Button type="button" onClick={props.onAdd}>
            <AppIcon icon={Add01Icon} aria-hidden data-icon="inline-start" />
            Add server
          </Button>
        </div>
      </header>

      {attentionCount > 0 && (
        <Alert variant="destructive">
          <AppIcon icon={Alert02Icon} aria-hidden />
          <AlertTitle>
            {attentionCount} {attentionCount === 1 ? 'server needs' : 'servers need'} attention
          </AlertTitle>
          <AlertDescription>
            <div className="flex flex-col items-start gap-3 sm:flex-row sm:items-center sm:justify-between">
              <p>Tool discovery needs attention. Review the result before you retry.</p>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => {
                  props.onQueryChange('')
                  props.onFilterChange('attention')
                  setSelectedId(null)
                }}
              >
                Review servers
                <AppIcon icon={ArrowRight01Icon} aria-hidden data-icon="inline-end" />
              </Button>
            </div>
          </AlertDescription>
        </Alert>
      )}

      <CandidateToolbar {...props} />

      {selected ? (
        <div className="grid min-w-0 items-start gap-5 lg:grid-cols-[minmax(280px,0.8fr)_minmax(0,1.2fr)]">
          <Frame spacing="sm" className="order-2 lg:order-1">
            <FrameHeader className="flex-row items-center justify-between gap-3">
              <FrameTitle>Server connections</FrameTitle>
              <Badge variant="outline">
                {orderedServers.length} of {props.allServers.length}
              </Badge>
            </FrameHeader>
            <FramePanel className="p-1">
              <ServerSelection
                servers={orderedServers}
                selectedId={selected.id}
                onSelect={(id) => {
                  setSelectedId(id)
                  if (window.matchMedia('(max-width: 1023px)').matches) {
                    document
                      .getElementById('selected-server-summary')
                      ?.scrollIntoView({ block: 'start' })
                  }
                }}
              />
            </FramePanel>
            <FrameFooter>
              <FrameDescription>Servers that need attention appear first.</FrameDescription>
            </FrameFooter>
          </Frame>
          <SelectedServer
            server={selected}
            onManage={props.onManage}
            onRefresh={props.onRefresh}
            refreshingId={props.refreshingId}
          />
        </div>
      ) : (
        <NoServers onAdd={props.onAdd} />
      )}
    </div>
  )
}

function ServerSelection({
  servers,
  selectedId,
  onSelect,
}: {
  servers: McpServerView[]
  selectedId: string
  onSelect: (id: string) => void
}) {
  return (
    <ul aria-label="Server connections" className="flex flex-col gap-1">
      {servers.map((server) => (
        <li key={server.id}>
          <Button
            type="button"
            variant={selectedId === server.id ? 'secondary' : 'ghost'}
            className="h-auto w-full justify-start gap-3 px-3 py-4 text-left whitespace-normal"
            aria-pressed={selectedId === server.id}
            aria-controls="selected-server-summary"
            onClick={() => onSelect(server.id)}
          >
            <ServerMark server={server} size="sm" />
            <span className="flex min-w-0 flex-1 flex-col gap-1.5">
              <span className="flex min-w-0 items-center justify-between gap-2">
                <span className="truncate">{server.display_name}</span>
                {needsAttention(server) && (
                  <AppIcon icon={Alert02Icon} aria-hidden className="text-destructive" />
                )}
                {selectedId === server.id && <AppIcon icon={ArrowRight01Icon} aria-hidden />}
              </span>
              <span className="text-muted-foreground truncate text-xs font-normal">
                {endpointHost(server)}
              </span>
              <span className="flex flex-wrap items-center gap-1.5">
                <RegistrationBadge server={server} />
                <DiscoveryBadge server={server} />
              </span>
            </span>
          </Button>
        </li>
      ))}
    </ul>
  )
}

function SelectedServer({
  server,
  onManage,
  onRefresh,
  refreshingId,
}: {
  server: McpServerView
} & Pick<CandidateProps, 'onManage' | 'onRefresh' | 'refreshingId'>) {
  return (
    <Frame
      spacing="lg"
      className="order-1 lg:order-2"
      id="selected-server-summary"
      role="region"
      aria-label={`${server.display_name} discovery summary`}
    >
      <FrameHeader className="flex-row items-center justify-between gap-3">
        <FrameTitle>Selected server</FrameTitle>
        <Button type="button" variant="ghost" size="sm" onClick={() => onManage(server)}>
          Manage server
          <AppIcon icon={ArrowRight01Icon} aria-hidden data-icon="inline-end" />
        </Button>
      </FrameHeader>
      <FramePanel className="flex flex-col gap-6">
        <div className="flex items-start gap-4">
          <ServerMark server={server} size="lg" />
          <div className="flex min-w-0 flex-1 flex-col gap-2">
            <h2 className="text-xl font-semibold tracking-tight">{server.display_name}</h2>
            <p className="text-muted-foreground font-mono text-xs break-all">{server.server_key}</p>
            <div className="flex flex-wrap items-center gap-2">
              <RegistrationBadge server={server} />
              <DiscoveryBadge server={server} />
            </div>
          </div>
        </div>
        {server.description && (
          <p className="text-muted-foreground text-sm leading-relaxed">{server.description}</p>
        )}
        <Separator />
        <DiscoveryResult
          server={server}
          onManage={onManage}
          onRefresh={onRefresh}
          refreshingId={refreshingId}
        />
        <Separator />
        <section aria-labelledby="connection-details-heading" className="flex flex-col gap-4">
          <h3 id="connection-details-heading" className="text-sm font-semibold">
            Connection details
          </h3>
          <dl className="grid gap-4 sm:grid-cols-2">
            <div className="flex min-w-0 flex-col gap-1 sm:col-span-2">
              <dt className="text-muted-foreground text-xs">Endpoint</dt>
              <dd className="font-mono text-xs leading-relaxed break-all">{server.server_url}</dd>
            </div>
            <div className="flex flex-col gap-1">
              <dt className="text-muted-foreground text-xs">Authentication</dt>
              <dd className="text-sm">{authLabel(server.auth_mode)}</dd>
            </div>
            <div className="flex flex-col gap-1">
              <dt className="text-muted-foreground text-xs">Transport</dt>
              <dd className="text-sm">
                {server.transport === 'streamable_http' ? 'Streamable HTTP' : server.transport}
              </dd>
            </div>
          </dl>
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => onManage(server, 'configuration')}
            >
              Edit configuration
            </Button>
            {server.auth_mode !== 'none' && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => onManage(server, 'credentials')}
              >
                Manage credentials
              </Button>
            )}
          </div>
        </section>
      </FramePanel>
      <FrameFooter>
        <FrameDescription>
          Discovery results show the latest recorded attempt for this server.
        </FrameDescription>
      </FrameFooter>
    </Frame>
  )
}

function DiscoveryResult({
  server,
  onManage,
  onRefresh,
  refreshingId,
}: { server: McpServerView } & Pick<CandidateProps, 'onManage' | 'onRefresh' | 'refreshingId'>) {
  const failed = needsAttention(server)
  const refreshing = refreshingId === server.id
  const lastSuccess = server.last_successful_discovery_at
    ? new Intl.DateTimeFormat('en-GB', {
        day: 'numeric',
        month: 'short',
        hour: '2-digit',
        minute: '2-digit',
        timeZone: 'UTC',
      }).format(new Date(server.last_successful_discovery_at)) + ' UTC'
    : 'No successful discovery'

  return (
    <section aria-labelledby="discovery-result-heading" className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h3 id="discovery-result-heading" className="text-sm font-semibold">
          Discovery result
        </h3>
        <Button
          type="button"
          variant={failed ? 'default' : 'outline'}
          size="sm"
          disabled={refreshingId !== null || server.status !== 'active'}
          onClick={() => onRefresh(server)}
        >
          <AppIcon
            icon={RefreshIcon}
            aria-hidden
            data-icon="inline-start"
            className={cn(refreshing && 'animate-spin motion-reduce:animate-none')}
          />
          {refreshing ? 'Refreshing…' : failed ? 'Retry discovery' : 'Refresh discovery'}
        </Button>
      </div>
      {failed && (
        <Alert variant="destructive">
          <AppIcon icon={Alert02Icon} aria-hidden />
          <AlertTitle>{discoveryLabel(server)}</AlertTitle>
          <AlertDescription>
            {server.last_error_summary ||
              'The latest discovery attempt failed. Review the server configuration and credentials before you retry.'}
          </AlertDescription>
        </Alert>
      )}
      {server.status !== 'active' && (
        <p className="text-muted-foreground text-sm">
          Enable this server in its settings to refresh discovery.
        </p>
      )}
      <dl className="grid gap-4 sm:grid-cols-2">
        <div className="flex flex-col gap-1">
          <dt className="text-muted-foreground text-xs">Last discovery attempt</dt>
          <dd className="text-sm">{discoveredAt(server)}</dd>
        </div>
        <div className="flex flex-col gap-1">
          <dt className="text-muted-foreground text-xs">Last successful discovery</dt>
          <dd className="text-sm">{lastSuccess}</dd>
        </div>
      </dl>
      <div className="bg-muted/50 flex flex-wrap items-center justify-between gap-3 rounded-lg p-4">
        <div className="flex items-baseline gap-3">
          <span className="text-3xl font-semibold tracking-tight tabular-nums">
            {server.last_tool_count ?? '—'}
          </span>
          <div className="flex flex-col gap-0.5">
            <span className="text-sm font-medium">Discovered tools</span>
            <span className="text-muted-foreground text-xs">
              {server.last_tool_count === null || server.last_tool_count === undefined
                ? 'No tool count recorded'
                : 'Last recorded count'}
            </span>
          </div>
        </div>
        <Button type="button" variant="outline" size="sm" onClick={() => onManage(server, 'tools')}>
          View tools
          <AppIcon icon={ArrowRight01Icon} aria-hidden data-icon="inline-end" />
        </Button>
      </div>
    </section>
  )
}
