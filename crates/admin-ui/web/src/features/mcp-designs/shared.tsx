import githubIcon from '@lobehub/icons-static-svg/icons/github.svg'
import notionIcon from '@lobehub/icons-static-svg/icons/notion.svg'
import exaIcon from '@lobehub/icons-static-svg/icons/exa.svg'
import figmaIcon from '@lobehub/icons-static-svg/icons/figma.svg'
import cloudflareIcon from '@lobehub/icons-static-svg/icons/cloudflare.svg'
import huggingFaceIcon from '@lobehub/icons-static-svg/icons/huggingface.svg'
import snowflakeIcon from '@lobehub/icons-static-svg/icons/snowflake.svg'
import {
  Add01Icon,
  ArrowRight01Icon,
  McpServerIcon,
  Search01Icon,
} from '@hugeicons/core-free-icons'
import { AppIcon } from '@/components/icons/app-icon'
import { IconTile } from '@/components/reui/icon-tile'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from '@/components/ui/empty'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { McpServerView } from '@/types/api'
import { discoveryLabel, needsAttention, type CandidateProps } from './model'

const marks: Record<string, string> = {
  github: githubIcon,
  notion: notionIcon,
  exa: exaIcon,
  figma: figmaIcon,
  'cloudflare-docs': cloudflareIcon,
  huggingface: huggingFaceIcon,
  snowflake: snowflakeIcon,
}

export function ServerMark({
  server,
  size = 'default',
}: {
  server: McpServerView
  size?: 'sm' | 'default' | 'lg'
}) {
  const mark = marks[server.server_key]
  return (
    <IconTile size={size} variant="outline" aria-hidden="true">
      {mark ? (
        <span
          className="size-5 bg-current"
          style={{
            maskImage: `url("${mark}")`,
            WebkitMaskImage: `url("${mark}")`,
            maskSize: 'contain',
            maskRepeat: 'no-repeat',
            maskPosition: 'center',
          }}
        />
      ) : (
        <AppIcon icon={McpServerIcon} />
      )}
    </IconTile>
  )
}

export function RegistrationBadge({ server }: { server: McpServerView }) {
  return (
    <Badge variant={server.status === 'active' ? 'success' : 'secondary'}>
      {server.status === 'active' ? 'Active' : 'Disabled'}
    </Badge>
  )
}

export function DiscoveryBadge({ server }: { server: McpServerView }) {
  const variant = needsAttention(server) ? 'destructive' : 'outline'
  return <Badge variant={variant}>{discoveryLabel(server)}</Badge>
}

export function CandidateToolbar({
  query,
  filter,
  onQueryChange,
  onFilterChange,
}: Pick<CandidateProps, 'query' | 'filter' | 'onQueryChange' | 'onFilterChange'>) {
  return (
    <div className="flex min-w-0 flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
      <InputGroup className="w-full sm:max-w-xs">
        <InputGroupInput
          aria-label="Search servers"
          placeholder="Search servers…"
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
        />
        <InputGroupAddon>
          <AppIcon icon={Search01Icon} aria-hidden />
        </InputGroupAddon>
      </InputGroup>
      <ToggleGroup
        type="single"
        variant="outline"
        size="sm"
        spacing={1}
        value={filter}
        aria-label="Filter servers"
        onValueChange={(value) => {
          if (value) onFilterChange(value as CandidateProps['filter'])
        }}
        className="grid w-full grid-cols-2 justify-start sm:flex sm:w-fit sm:flex-wrap"
      >
        <ToggleGroupItem value="all">All servers</ToggleGroupItem>
        <ToggleGroupItem value="attention">Needs attention</ToggleGroupItem>
        <ToggleGroupItem value="active">Active</ToggleGroupItem>
        <ToggleGroupItem value="disabled">Disabled</ToggleGroupItem>
      </ToggleGroup>
    </div>
  )
}

export function NoServers({ onAdd }: { onAdd: () => void }) {
  return (
    <Empty>
      <EmptyHeader>
        <EmptyTitle>No servers to show</EmptyTitle>
        <EmptyDescription>
          Change your search or filter, or connect a new MCP server.
        </EmptyDescription>
      </EmptyHeader>
      <EmptyContent>
        <Button type="button" onClick={onAdd}>
          <AppIcon icon={Add01Icon} aria-hidden data-icon="inline-start" />
          Add server
        </Button>
      </EmptyContent>
    </Empty>
  )
}

export function CatalogPrompt({ onCatalog }: { onCatalog: () => void }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Connect your next tool</CardTitle>
        <CardDescription>
          Start with a recommended server or configure your own endpoint.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-wrap items-center justify-between gap-4">
        <div className="flex items-center gap-2">
          <span className="text-muted-foreground text-sm">
            From source code to shared knowledge.
          </span>
        </div>
        <Button type="button" variant="outline" onClick={onCatalog}>
          Browse catalog
          <AppIcon icon={ArrowRight01Icon} aria-hidden data-icon="inline-end" />
        </Button>
      </CardContent>
    </Card>
  )
}
