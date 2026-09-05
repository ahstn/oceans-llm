import { useState, type CSSProperties, type FormEvent, type ReactNode } from 'react'
import {
  ArrowDown01Icon,
  Configuration01Icon,
  ShieldKeyIcon,
  ToolsIcon,
  ViewIcon,
} from '@hugeicons/core-free-icons'
import { AppIcon } from '@/components/icons/app-icon'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
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
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Separator } from '@/components/ui/separator'
import {
  Sidebar,
  SidebarContent,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
} from '@/components/ui/sidebar'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { McpServerView } from '@/types/api'
import {
  authLabel,
  createPreviewServer,
  discoveredAt,
  formatTimestamp,
  sampleCatalog,
  sampleTools,
  type DetailSection,
} from './model'
import { DiscoveryBadge, RegistrationBadge, ServerMark } from './shared'

interface PreviewDialogProps {
  server: McpServerView | null
  section: DetailSection
  onSectionChange: (section: DetailSection) => void
  onClose: () => void
  mode: 'add' | 'catalog' | null
  onModeChange: (mode: 'add' | 'catalog' | null) => void
  onAdd: (server: McpServerView) => void
  onUpdate: (server: McpServerView) => void
  existingServerKeys: string[]
}

const sections = [
  { value: 'overview', label: 'Overview', icon: ViewIcon },
  { value: 'configuration', label: 'Configuration', icon: Configuration01Icon },
  { value: 'tools', label: 'Tools', icon: ToolsIcon },
  { value: 'credentials', label: 'Credentials', icon: ShieldKeyIcon },
] as const

export function PreviewDialogs(props: PreviewDialogProps) {
  return (
    <>
      <Dialog
        open={Boolean(props.server)}
        onOpenChange={(open) => {
          if (!open) props.onClose()
        }}
      >
        <DialogContent className="h-[min(760px,90dvh)] min-h-0 min-w-0 gap-0 overflow-hidden p-0 sm:max-w-4xl">
          {props.server ? (
            <ServerDetails {...props} server={props.server} />
          ) : (
            <DialogTitle>Manage server</DialogTitle>
          )}
        </DialogContent>
      </Dialog>
      <Dialog
        open={Boolean(props.mode)}
        onOpenChange={(open) => {
          if (!open) props.onModeChange(null)
        }}
      >
        <DialogContent className="max-h-[90dvh] min-w-0 overflow-y-auto sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>{props.mode === 'catalog' ? 'Server catalog' : 'Add server'}</DialogTitle>
            <DialogDescription>
              {props.mode === 'catalog'
                ? 'Choose a sample template to add to this preview.'
                : 'Set up a sample server for this design preview.'}
            </DialogDescription>
          </DialogHeader>
          {props.mode === 'catalog' ? (
            <Catalog onAdd={props.onAdd} existingServerKeys={props.existingServerKeys} />
          ) : props.mode === 'add' ? (
            <ServerEditor onSave={props.onAdd} onDone={() => props.onModeChange(null)} />
          ) : null}
        </DialogContent>
      </Dialog>
    </>
  )
}

function ServerDetails({
  server,
  section,
  onSectionChange,
  onUpdate,
}: PreviewDialogProps & { server: McpServerView }) {
  return (
    <SidebarProvider
      className="h-full min-h-0 min-w-0 items-start overflow-hidden"
      style={{ '--sidebar-width': '11rem' } as CSSProperties}
    >
      <Sidebar collapsible="none" className="hidden self-stretch border-r md:flex">
        <SidebarContent className="px-3 py-5">
          <p className="text-muted-foreground px-2 pb-3 text-xs">SERVER SETTINGS</p>
          <SidebarMenu>
            {sections.map((item) => (
              <SidebarMenuItem key={item.value}>
                <SidebarMenuButton
                  type="button"
                  isActive={section === item.value}
                  onClick={() => onSectionChange(item.value)}
                >
                  <AppIcon icon={item.icon} aria-hidden />
                  <span>{item.label}</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            ))}
          </SidebarMenu>
        </SidebarContent>
      </Sidebar>
      <div className="flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        <DialogHeader className="min-w-0 shrink-0 p-6 pr-12">
          <div className="flex min-w-0 items-center gap-3">
            <ServerMark server={server} size="lg" />
            <div className="flex min-w-0 flex-col gap-2">
              <DialogTitle className="truncate">{server.display_name}</DialogTitle>
              <DialogDescription className="truncate font-mono text-xs">
                {server.server_key}
              </DialogDescription>
            </div>
          </div>
          <div className="flex flex-wrap gap-2 pt-2">
            <RegistrationBadge server={server} />
            <DiscoveryBadge server={server} />
          </div>
        </DialogHeader>
        <div className="min-w-0 shrink-0 overflow-x-auto px-6 pb-4 md:hidden">
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            value={section}
            aria-label="Server sections"
            onValueChange={(value) => {
              if (value) onSectionChange(value as DetailSection)
            }}
          >
            {sections.map((item) => (
              <ToggleGroupItem key={item.value} value={item.value}>
                {item.label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </div>
        <Separator />
        <div className="min-h-0 min-w-0 flex-1 overflow-y-auto p-6">
          {section === 'overview' ? <Overview server={server} /> : null}
          {section === 'configuration' ? (
            <ServerEditor
              key={server.id}
              server={server}
              onSave={onUpdate}
              onDone={() => onSectionChange('overview')}
            />
          ) : null}
          {section === 'tools' ? <SampleTools server={server} /> : null}
          {section === 'credentials' ? <Credentials server={server} /> : null}
        </div>
      </div>
    </SidebarProvider>
  )
}

function Overview({ server }: { server: McpServerView }) {
  return (
    <div className="flex min-w-0 flex-col gap-5">
      {server.last_error_summary ? (
        <Alert variant="destructive">
          <AlertTitle>Discovery needs attention</AlertTitle>
          <AlertDescription>{server.last_error_summary}</AlertDescription>
        </Alert>
      ) : null}
      <div className="flex flex-col gap-2">
        <h3 className="font-medium">About this server</h3>
        <p className="text-muted-foreground text-sm">
          {server.description || 'A custom MCP endpoint registered with the gateway.'}
        </p>
      </div>
      <dl className="grid min-w-0 gap-x-6 sm:grid-cols-2">
        <Detail label="Endpoint" className="sm:col-span-2">
          <span className="font-mono text-xs break-all">{server.server_url}</span>
        </Detail>
        <Detail label="Authentication">{authLabel(server.auth_mode)}</Detail>
        <Detail label="Transport">{server.transport.replaceAll('_', ' ')}</Detail>
        <Detail label="Last discovery">{discoveredAt(server)}</Detail>
        <Detail label="Last success">{formatTimestamp(server.last_successful_discovery_at)}</Detail>
        <Detail label="Discovered tools">{server.last_tool_count ?? 'Not discovered'}</Detail>
        <Detail label="Request timeout">{server.timeout_ms.toLocaleString()} ms</Detail>
      </dl>
    </div>
  )
}

function Detail({
  label,
  children,
  className,
}: {
  label: string
  children: ReactNode
  className?: string
}) {
  return (
    <div className={className}>
      <Separator />
      <div className="flex min-w-0 flex-col gap-1 py-3">
        <dt className="text-muted-foreground text-xs">{label}</dt>
        <dd className="min-w-0 text-sm">{children}</dd>
      </div>
    </div>
  )
}

function ServerEditor({
  server,
  onSave,
  onDone,
}: {
  server?: McpServerView
  onSave: (server: McpServerView) => void
  onDone: () => void
}) {
  const [form, setForm] = useState({
    display_name: server?.display_name ?? '',
    server_url: server?.server_url ?? '',
    status: server?.status ?? 'active',
  })
  function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const now = new Date().toISOString()
    const values = {
      ...form,
      display_name: form.display_name.trim(),
      server_url: form.server_url.trim(),
    }
    onSave(
      server
        ? {
            ...server,
            ...values,
            disabled_at: form.status === 'disabled' ? (server.disabled_at ?? now) : null,
            updated_at: now,
          }
        : createPreviewServer({
            ...values,
            server_key:
              values.display_name
                .toLowerCase()
                .replace(/[^a-z0-9]+/g, '-')
                .replace(/^-|-$/g, '') || 'custom-server',
          }),
    )
    if (server) onDone()
  }
  return (
    <form className="flex min-w-0 flex-col gap-6" onSubmit={save}>
      <FieldGroup>
        {(['display_name', 'server_url'] as const).map((field) => (
          <Field key={field}>
            <FieldLabel htmlFor={`preview-${field}`}>
              {field === 'display_name' ? 'Display name' : 'Server URL'}
            </FieldLabel>
            <Input
              id={`preview-${field}`}
              required
              className="min-w-0"
              type={field === 'server_url' ? 'url' : 'text'}
              pattern={field === 'server_url' ? 'https?://.+' : undefined}
              value={form[field]}
              onChange={(event) => setForm({ ...form, [field]: event.target.value })}
            />
          </Field>
        ))}
        {server ? (
          <Field>
            <FieldLabel>Registration</FieldLabel>
            <ToggleGroup
              type="single"
              variant="outline"
              value={form.status}
              aria-label="Server registration"
              onValueChange={(value) => {
                if (value) setForm({ ...form, status: value })
              }}
            >
              <ToggleGroupItem value="active">Active</ToggleGroupItem>
              <ToggleGroupItem value="disabled">Disabled</ToggleGroupItem>
            </ToggleGroup>
          </Field>
        ) : null}
      </FieldGroup>
      <Separator />
      <div className="flex justify-end gap-2">
        <Button type="button" variant="outline" onClick={onDone}>
          Cancel
        </Button>
        <Button type="submit" disabled={!form.display_name.trim()}>
          {server ? 'Save changes' : 'Add server'}
        </Button>
      </div>
    </form>
  )
}

function SampleTools({ server }: { server: McpServerView }) {
  if (server.last_tool_count == null || server.last_tool_count === 0)
    return (
      <Empty>
        <EmptyHeader>
          <EmptyTitle>No tools discovered</EmptyTitle>
          <EmptyDescription>
            This sample server has no saved tool list. Open another server to inspect sample
            schemas.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  return (
    <div className="flex max-w-full min-w-0 flex-col gap-4 overflow-hidden">
      <div className="flex flex-col gap-1">
        <h3 className="font-medium">Sample tools</h3>
        <p className="text-muted-foreground text-sm">
          Three illustrative tool schemas. These are not the service’s actual tool definitions.
        </p>
      </div>
      {sampleTools.map((tool) => (
        <Collapsible key={tool.name} className="max-w-full min-w-0">
          <Separator />
          <CollapsibleTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              className="h-auto w-full min-w-0 justify-between px-0 py-4 text-left"
            >
              <span className="flex min-w-0 flex-col gap-1">
                <span className="truncate font-mono text-sm">{tool.name}</span>
                <span className="text-muted-foreground truncate text-xs font-normal">
                  {tool.description}
                </span>
              </span>
              <AppIcon icon={ArrowDown01Icon} aria-hidden data-icon="inline-end" />
            </Button>
          </CollapsibleTrigger>
          <CollapsibleContent className="max-w-full min-w-0 overflow-hidden pb-4">
            <pre className="bg-muted max-w-full overflow-x-auto rounded-md p-4 text-xs">
              <code>{JSON.stringify(tool.schema, null, 2)}</code>
            </pre>
          </CollapsibleContent>
        </Collapsible>
      ))}
    </div>
  )
}

function Credentials({ server }: { server: McpServerView }) {
  return (
    <div className="flex min-w-0 flex-col gap-5">
      <div className="flex flex-col gap-1">
        <h3 className="font-medium">Credentials</h3>
        <p className="text-muted-foreground text-sm">
          Review the authentication method used for this sample server.
        </p>
      </div>
      <dl>
        <Detail label="Authentication">{authLabel(server.auth_mode)}</Detail>
        <Detail label="Credential bindings">
          <Badge variant="outline">Sample configuration only</Badge>
        </Detail>
      </dl>
      <Alert>
        <AlertTitle>
          {server.auth_mode === 'none'
            ? 'No authentication required'
            : 'Credentials stay outside this preview'}
        </AlertTitle>
        <AlertDescription>
          {server.auth_mode === 'none'
            ? 'This sample endpoint is configured without authentication.'
            : 'The live server dialog manages user and account credential bindings. This preview does not request, store, or validate secrets.'}
        </AlertDescription>
      </Alert>
    </div>
  )
}

function Catalog({
  onAdd,
  existingServerKeys,
}: {
  onAdd: (server: McpServerView) => void
  existingServerKeys: string[]
}) {
  const registered = new Set(existingServerKeys)
  return (
    <div className="flex min-w-0 flex-col gap-1">
      {sampleCatalog.map((server) => (
        <div key={server.id}>
          <Separator />
          <div className="flex min-w-0 items-start gap-3 py-4">
            <ServerMark server={server} />
            <div className="flex min-w-0 flex-1 flex-col gap-1">
              <p className="font-medium">{server.display_name}</p>
              <p className="text-muted-foreground text-xs">{server.description}</p>
              <p className="text-muted-foreground text-xs">{authLabel(server.auth_mode)}</p>
            </div>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={registered.has(server.server_key)}
              onClick={() => onAdd(createPreviewServer(server))}
            >
              {registered.has(server.server_key) ? 'Added' : 'Add'}
              <span className="sr-only"> {server.display_name}</span>
            </Button>
          </div>
        </div>
      ))}
    </div>
  )
}
