import { useEffect, useState } from 'react'

import { AgentHarnessLabel } from '@/components/icons/agent-harness-icon'
import {
  CodeBlock,
  CodeBlockCopyButton,
  CodeBlockHeader,
  CodeBlockTitle,
} from '@/components/reui/code-block/code-block'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Separator } from '@/components/ui/separator'
import { Spinner } from '@/components/ui/spinner'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { McpConnectionInfoPayload, McpToolsetView } from '@/types/api'

interface ToolsetConnectionDialogProps {
  toolset: McpToolsetView
  loadConnectionInfo: () => Promise<McpConnectionInfoPayload>
}

type ConnectionState =
  | { status: 'loading' }
  | { status: 'error' }
  | { status: 'ready'; info: McpConnectionInfoPayload }

export function ToolsetConnectionDialog({
  toolset,
  loadConnectionInfo,
}: ToolsetConnectionDialogProps) {
  const [open, setOpen] = useState(false)
  const [attempt, setAttempt] = useState(0)
  const [state, setState] = useState<ConnectionState>({ status: 'loading' })

  useEffect(() => {
    if (!open) return
    let cancelled = false
    setState({ status: 'loading' })
    async function load() {
      try {
        const info = await loadConnectionInfo()
        if (!cancelled) setState({ status: 'ready', info })
      } catch {
        if (!cancelled) setState({ status: 'error' })
      }
    }
    void load()
    return () => {
      cancelled = true
    }
  }, [open, attempt, loadConnectionInfo])

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        setState({ status: 'loading' })
        setOpen(nextOpen)
      }}
    >
      <DialogTrigger asChild>
        <Button type="button" variant="ghost" size="sm">
          Connection Info
        </Button>
      </DialogTrigger>
      <DialogContent
        className="flex max-h-[min(880px,calc(100dvh-2rem))] max-w-[calc(100vw-2rem)] min-w-0 flex-col overflow-hidden sm:max-w-[min(920px,calc(100vw-2rem))] md:min-w-[35vw]"
        data-testid="toolset-connection-dialog"
      >
        <DialogHeader className="min-w-0 pr-8">
          <DialogTitle>Connection Info</DialogTitle>
          <DialogDescription className="wrap-anywhere">
            Connect your client to use {toolset.display_name}.
          </DialogDescription>
        </DialogHeader>
        {state.status === 'loading' ? (
          <div role="status" className="flex items-center gap-2 py-8">
            <Spinner aria-hidden="true" />
            Loading connection info…
          </div>
        ) : state.status === 'error' ? (
          <Alert variant="destructive">
            <AlertTitle>Connection info could not be loaded</AlertTitle>
            <AlertDescription>
              Check the gateway connection settings, then try again.
            </AlertDescription>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="mt-3 w-fit"
              onClick={() => setAttempt((value) => value + 1)}
            >
              Retry connection info
            </Button>
          </Alert>
        ) : (
          <ConnectionSections toolset={toolset} info={state.info} />
        )}
      </DialogContent>
    </Dialog>
  )
}

function ConnectionSections({
  toolset,
  info,
}: {
  toolset: McpToolsetView
  info: McpConnectionInfoPayload
}) {
  const [activeKey, setActiveKey] = useState('connection')
  const activeConfig = info.client_configurations.find((config) => config.key === activeKey)

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-4 overflow-hidden">
      <nav
        aria-label="Connection info sections"
        className="flex shrink-0 flex-wrap items-center gap-3"
      >
        <ToggleGroup
          type="single"
          value={activeKey}
          onValueChange={(value) => {
            if (value) setActiveKey(value)
          }}
          variant="outline"
          size="sm"
          spacing={1}
          className="max-w-full min-w-0 flex-wrap"
          aria-label="Connection setup"
        >
          <ToggleGroupItem value="connection">
            <span className="px-2">Connection</span>
          </ToggleGroupItem>
          {info.client_configurations.map((config) => (
            <ToggleGroupItem key={config.key} value={config.key}>
              <AgentHarnessLabel className="px-2" harnessKey={config.key}>
                {config.label}
              </AgentHarnessLabel>
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </nav>
      <div
        className="flex min-h-0 max-w-full min-w-0 flex-1 flex-col overflow-hidden"
        data-testid="toolset-connection-panel"
      >
        <div
          className="min-h-0 max-w-full min-w-0 overflow-y-auto pr-1 pb-1"
          data-testid="toolset-connection-scroll"
        >
          {activeConfig ? (
            <ClientSetup config={activeConfig} />
          ) : (
            <ConnectionOverview toolset={toolset} endpoint={info.endpoint} />
          )}
        </div>
      </div>
    </div>
  )
}

function ConnectionOverview({ toolset, endpoint }: { toolset: McpToolsetView; endpoint: string }) {
  return (
    <div className="flex min-w-0 flex-col gap-4">
      <div className="flex flex-col gap-2">
        <h3 className="text-sm font-medium">Gateway connection</h3>
        <p className="text-muted-foreground text-sm">
          Connect your client to the gateway, then grant {toolset.display_name} to the API key or
          its owner in Manage access. The client can use all tools granted to that key.
        </p>
      </div>
      {toolset.status !== 'active' ? (
        <Alert>
          <AlertTitle>This tool set is disabled</AlertTitle>
          <AlertDescription>
            It contributes no tools until enabled. Other grants can still provide tools to the same
            API key.
          </AlertDescription>
        </Alert>
      ) : null}
      <ConnectionCode title="MCP endpoint" code={endpoint} />
      <dl className="flex min-w-0 flex-col gap-4 text-sm">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <dt className="text-muted-foreground">Transport</dt>
          <dd>
            <Badge variant="secondary">Streamable HTTP</Badge>
          </dd>
        </div>
        <div className="flex flex-col gap-1">
          <dt className="text-muted-foreground">Authentication</dt>
          <dd>Authorization: Bearer &lt;your Oceans API key&gt;</dd>
        </div>
      </dl>
      <Separator />
      <div className="text-muted-foreground flex flex-col gap-3 text-sm">
        <p>Use an API key owned by a user or service account with access to this tool set.</p>
        <p>
          Tool sets share this endpoint. Save changes in the navigator to update the tools available
          to clients.
        </p>
        <p>
          Clients discover and run permitted tools through <code>search_tools</code>,{' '}
          <code>describe_tool</code>, and <code>call_tool</code>.
        </p>
      </div>
    </div>
  )
}

function ClientSetup({
  config,
}: {
  config: McpConnectionInfoPayload['client_configurations'][number]
}) {
  return (
    <div className="flex max-w-full min-w-0 flex-col gap-4 overflow-hidden">
      <h3 className="text-sm font-medium">{config.label} setup</h3>
      {config.setup.length > 0 ? (
        <dl className="flex min-w-0 flex-col divide-y text-sm">
          {config.setup.map((item) => (
            <div
              key={`${item.label}:${item.value}`}
              className="grid min-w-0 gap-1 py-3 sm:grid-cols-[8rem_minmax(0,1fr)] sm:gap-4"
            >
              <dt className="font-medium">{item.label}</dt>
              <dd className="text-muted-foreground min-w-0 wrap-anywhere">
                {item.href ? (
                  <a
                    href={item.href}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="font-mono text-xs underline underline-offset-4"
                  >
                    {item.value}
                  </a>
                ) : (
                  item.value
                )}
              </dd>
            </div>
          ))}
        </dl>
      ) : null}
      {config.blocks.map((block) => (
        <ConnectionCode
          key={`${block.filename}:${block.label}`}
          title={block.filename}
          language={configLanguage(block.filename)}
          code={block.content}
        />
      ))}
      {config.notes.length > 0 ? (
        <div className="text-muted-foreground flex min-w-0 flex-col gap-3 text-sm wrap-anywhere">
          {config.notes.map((note) => (
            <p key={note}>{note}</p>
          ))}
        </div>
      ) : null}
    </div>
  )
}

function configLanguage(filename: string) {
  if (filename.endsWith('.json') || filename.endsWith('.jsonc')) return 'json'
  if (filename.endsWith('.toml')) return 'toml'
  if (filename.endsWith('.sh')) return 'bash'
  return 'text'
}

function ConnectionCode({
  title,
  code,
  language = 'text',
}: {
  title: string
  code: string
  language?: string
}) {
  return (
    <CodeBlock code={code} language={language} maxLines={14}>
      <CodeBlockHeader>
        <CodeBlockTitle>{title}</CodeBlockTitle>
        <CodeBlockCopyButton className="ml-auto" labels={{ copy: `Copy ${title}` }} />
      </CodeBlockHeader>
    </CodeBlock>
  )
}
