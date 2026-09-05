import { useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Toaster, toast } from 'sonner'
import { Moon02Icon, Sun03Icon } from '@hugeicons/core-free-icons'
import { AppIcon } from '@/components/icons/app-icon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { TooltipProvider } from '@/components/ui/tooltip'

import type { McpServerView } from '@/types/api'
import '@/styles/globals.css'
import { PreviewShell } from './preview-shell'
import { RegistryCandidate } from './registry-candidate'
import { LibraryCandidate } from './library-candidate'
import { OperationsCandidate } from './operations-candidate'
import { PreviewDialogs } from './preview-dialogs'
import {
  filterServers,
  sampleServers,
  type CandidateProps,
  type DetailSection,
  type ServerFilter,
} from './model'

type Candidate = 'registry' | 'library' | 'operations'
const candidateNames: Record<Candidate, string> = {
  registry: 'A · Registry',
  library: 'B · Library',
  operations: 'C · Operations',
}
const candidateNotes: Record<Candidate, string> = {
  registry:
    'A compact inventory for everyday administration. Compare discovery results, sort the list, and manage a server in one place.',
  library:
    'A service library for exploring connections. Descriptions and tool counts make each integration easier to understand.',
  operations:
    'A discovery workspace for investigating failures. Keep the server list in view while checking the selected connection.',
}

function initialCandidate(): Candidate {
  const value = new URLSearchParams(window.location.search).get('candidate')
  return value === 'library' || value === 'operations' ? value : 'registry'
}

function Preview() {
  const [candidate, setCandidate] = useState<Candidate>(initialCandidate)
  const [servers, setServers] = useState(sampleServers)
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<ServerFilter>('all')
  const [dark, setDark] = useState(true)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [section, setSection] = useState<DetailSection>('overview')
  const [mode, setMode] = useState<'add' | 'catalog' | null>(null)
  const [refreshingId, setRefreshingId] = useState<string | null>(null)

  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark)
  }, [dark])
  useEffect(() => {
    if (!refreshingId) return
    const timer = window.setTimeout(() => {
      setRefreshingId(null)
      toast.info('Sample discovery complete. Displayed results come from the preview fixtures.')
    }, 650)
    return () => window.clearTimeout(timer)
  }, [refreshingId])

  function changeCandidate(value: string) {
    if (!(value in candidateNames)) return
    setCandidate(value as Candidate)
    window.history.replaceState(null, '', `?candidate=${value}`)
  }
  function addServer(server: McpServerView) {
    if (servers.some((item) => item.server_key === server.server_key)) {
      toast.error('This server is already in the sample workspace.')
      return
    }
    setServers((items) => [...items, server])
    setMode(null)
    setQuery('')
    setFilter('all')
    toast.success('Server added to this preview.')
  }
  const props: CandidateProps = {
    servers: filterServers(servers, query, filter),
    allServers: servers,
    query,
    filter,
    onQueryChange: setQuery,
    onFilterChange: setFilter,
    onManage: (server, nextSection = 'overview') => {
      setSelectedId(server.id)
      setSection(nextSection)
    },
    onAdd: () => setMode('add'),
    onCatalog: () => setMode('catalog'),
    onRefresh: (server) => {
      if (server.status === 'active' && !refreshingId) setRefreshingId(server.id)
    },
    refreshingId,
  }
  return (
    <TooltipProvider>
      <PreviewShell>
        <PreviewControls
          candidate={candidate}
          dark={dark}
          onCandidateChange={changeCandidate}
          onThemeChange={() => setDark(!dark)}
          onReset={() => {
            setServers(sampleServers)
            setQuery('')
            setFilter('all')
            setSelectedId(null)
            setMode(null)
          }}
        />
        {candidate === 'registry' ? (
          <RegistryCandidate {...props} />
        ) : candidate === 'library' ? (
          <LibraryCandidate {...props} />
        ) : (
          <OperationsCandidate {...props} />
        )}
        <p className="text-muted-foreground text-xs">
          Interactive design preview · Sample data · Changes stay in this page and reset on reload.
        </p>
        <PreviewDialogs
          existingServerKeys={servers.map((server) => server.server_key)}
          server={servers.find((server) => server.id === selectedId) ?? null}
          section={section}
          onSectionChange={setSection}
          onClose={() => setSelectedId(null)}
          mode={mode}
          onModeChange={setMode}
          onAdd={addServer}
          onUpdate={(server) => {
            setServers((items) => items.map((item) => (item.id === server.id ? server : item)))
            toast.success('Sample configuration updated.')
          }}
        />
      </PreviewShell>
      <Toaster theme={dark ? 'dark' : 'light'} />
    </TooltipProvider>
  )
}

function PreviewControls({
  candidate,
  dark,
  onCandidateChange,
  onThemeChange,
  onReset,
}: {
  candidate: Candidate
  dark: boolean
  onCandidateChange: (value: string) => void
  onThemeChange: () => void
  onReset: () => void
}) {
  return (
    <div className="flex flex-col gap-3 border-b pb-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <Badge variant="outline">Design study</Badge>
          <span className="text-muted-foreground text-xs">MCP servers / 03 candidates</span>
        </div>
        <div className="flex items-center gap-2">
          <Button type="button" variant="ghost" size="sm" onClick={onReset}>
            Reset sample
          </Button>
          <Button
            type="button"
            variant="outline"
            size="icon-sm"
            aria-label={dark ? 'Use light theme' : 'Use dark theme'}
            onClick={onThemeChange}
          >
            <AppIcon icon={dark ? Sun03Icon : Moon02Icon} aria-hidden />
          </Button>
        </div>
      </div>
      <ToggleGroup
        type="single"
        value={candidate}
        onValueChange={onCandidateChange}
        variant="outline"
        aria-label="Design candidate"
        className="flex-wrap justify-start"
      >
        {Object.entries(candidateNames).map(([value, name]) => (
          <ToggleGroupItem key={value} value={value}>
            {name}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
      <p className="text-muted-foreground max-w-3xl text-sm">{candidateNotes[candidate]}</p>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(<Preview />)
