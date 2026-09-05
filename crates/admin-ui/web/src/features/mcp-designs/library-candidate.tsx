import {
  Add01Icon,
  ArrowRight01Icon,
  RefreshIcon,
  ShieldKeyIcon,
  ToolsIcon,
} from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { IconTile } from '@/components/reui/icon-tile'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Separator } from '@/components/ui/separator'
import type { McpServerView } from '@/types/api'
import { cn } from '@/lib/utils'

import { authLabel, endpointHost, needsAttention, type CandidateProps } from './model'
import {
  CandidateToolbar,
  CatalogPrompt,
  DiscoveryBadge,
  NoServers,
  RegistrationBadge,
  ServerMark,
} from './shared'

export function LibraryCandidate(props: CandidateProps) {
  const activeCount = props.allServers.filter((server) => server.status === 'active').length
  const attention = props.allServers.filter(needsAttention)

  return (
    <div className="flex min-w-0 flex-col gap-7">
      <div className="grid gap-6 lg:grid-cols-[minmax(0,1.8fr)_minmax(260px,1fr)]">
        <div className="flex flex-col items-start justify-center gap-5 py-3">
          <p className="text-primary text-xs font-medium tracking-[0.16em] uppercase">
            Server library
          </p>
          <div className="flex flex-col gap-3">
            <h1 className="text-3xl leading-tight font-semibold tracking-tight sm:text-4xl">
              Your tools, connected.
            </h1>
            <p className="text-muted-foreground max-w-lg text-sm leading-relaxed">
              Bring your services into Oceans. Manage each connection, discover its tools, and keep
              access under control.
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-4">
            <Button type="button" onClick={props.onAdd}>
              <AppIcon icon={Add01Icon} data-icon="inline-start" aria-hidden />
              Add server
            </Button>
            <p className="text-muted-foreground text-xs">
              <span className="text-foreground font-medium">{props.allServers.length}</span>{' '}
              registered · <span className="text-foreground font-medium">{activeCount}</span> active
            </p>
          </div>
        </div>
        <CatalogPrompt onCatalog={props.onCatalog} />
      </div>

      <Separator />

      {attention.length > 0 ? (
        <Alert>
          <AppIcon icon={ShieldKeyIcon} aria-hidden />
          <AlertTitle>
            {attention.length === 1
              ? `${attention[0].display_name} needs attention`
              : `${attention.length} servers need attention`}
          </AlertTitle>
          <AlertDescription>
            <div className="flex w-full flex-wrap items-center justify-between gap-3">
              <p>Discovery failed. Review the connection before using its saved tool list.</p>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => props.onManage(attention[0], 'overview')}
              >
                Review connection
                <AppIcon icon={ArrowRight01Icon} data-icon="inline-end" aria-hidden />
              </Button>
            </div>
          </AlertDescription>
        </Alert>
      ) : null}

      <section aria-label="Registered servers" className="flex min-w-0 flex-col gap-5">
        <CandidateToolbar
          query={props.query}
          filter={props.filter}
          onQueryChange={props.onQueryChange}
          onFilterChange={props.onFilterChange}
        />
        {props.servers.length > 0 ? (
          <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
            {props.servers.map((server) => (
              <LibraryServerCard key={server.id} server={server} {...props} />
            ))}
          </div>
        ) : (
          <NoServers onAdd={props.onAdd} />
        )}
      </section>
    </div>
  )
}

function LibraryServerCard({
  server,
  onManage,
  onRefresh,
  refreshingId,
}: Pick<CandidateProps, 'onManage' | 'onRefresh' | 'refreshingId'> & {
  server: McpServerView
}) {
  const refreshing = refreshingId === server.id
  const hasSavedTools = server.last_tool_count !== null && server.last_tool_count !== undefined

  return (
    <Card className="min-w-0">
      <CardHeader className="gap-4">
        <ServerMark server={server} size="lg" />
        <CardAction>
          <RegistrationBadge server={server} />
        </CardAction>
        <div className="flex min-w-0 flex-col gap-1.5">
          <CardTitle>{server.display_name}</CardTitle>
          <CardDescription className="min-h-10">
            {server.description || 'A custom MCP server registered with your gateway.'}
          </CardDescription>
        </div>
      </CardHeader>

      <CardContent className="flex flex-1 flex-col gap-4">
        <div className="bg-muted/50 flex items-center gap-3 rounded-lg p-3">
          <IconTile variant="outline" size="sm">
            <AppIcon icon={ToolsIcon} aria-hidden />
          </IconTile>
          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
            <p className="text-sm font-medium tabular-nums">
              {hasSavedTools ? `${server.last_tool_count} tools` : 'No tools discovered'}
            </p>
            <p className="text-muted-foreground text-xs">
              {hasSavedTools ? 'Saved discovery result' : 'Run discovery to find tools'}
            </p>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label={`View ${server.display_name} tools`}
            onClick={() => onManage(server, 'tools')}
          >
            <AppIcon icon={ArrowRight01Icon} data-icon="inline-end" aria-hidden />
          </Button>
        </div>

        <dl className="flex flex-col gap-3 text-xs">
          <div className="flex items-center justify-between gap-3">
            <dt className="text-muted-foreground">Discovery</dt>
            <dd>
              <DiscoveryBadge server={server} />
            </dd>
          </div>
          <div className="flex items-center justify-between gap-3">
            <dt className="text-muted-foreground">Authentication</dt>
            <dd>{authLabel(server.auth_mode)}</dd>
          </div>
        </dl>
        <p
          className="text-muted-foreground mt-auto truncate font-mono text-[11px]"
          title={server.server_url}
        >
          {endpointHost(server)}
        </p>
      </CardContent>

      <CardFooter className="justify-between gap-3">
        <Button type="button" variant="outline" size="sm" onClick={() => onManage(server)}>
          Manage server
          <AppIcon icon={ArrowRight01Icon} data-icon="inline-end" aria-hidden />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          aria-label={`Refresh ${server.display_name} discovery`}
          title="Refresh discovery"
          disabled={refreshing || server.status === 'disabled'}
          onClick={() => onRefresh(server)}
        >
          <AppIcon
            icon={RefreshIcon}
            data-icon="inline-start"
            className={cn(refreshing && 'motion-safe:animate-spin')}
            aria-hidden
          />
        </Button>
      </CardFooter>
    </Card>
  )
}
