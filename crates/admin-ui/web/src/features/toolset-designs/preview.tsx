import { useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Toaster, toast } from 'sonner'
import { Moon02Icon, Sun03Icon } from '@hugeicons/core-free-icons'
import { AppIcon } from '@/components/icons/app-icon'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { TooltipProvider } from '@/components/ui/tooltip'
import type { McpToolsetView } from '@/types/api'
import { PreviewShell } from '../mcp-designs/preview-shell'
import { DirectoryCandidate } from './directory-candidate'
import { WorkbenchCandidate } from './workbench-candidate'
import { GuidedCandidate } from './guided-candidate'
import { ToolsetDialogs, type DialogMode } from './preview-dialogs'
import {
  filterToolsets,
  initialCandidate,
  initialMemberships,
  membershipIsDirty,
  sampleToolsets,
  type CatalogState,
  type ToolsetCandidate,
  type ToolsetCandidateProps,
  type ToolsetDetails,
  type ToolsetFilter,
} from './model'
import '@/styles/globals.css'

const candidateNames = {
  directory: 'A · Directory',
  workbench: 'B · Workbench',
  guided: 'C · Guided Builder',
}
const candidateNotes = {
  directory:
    'Familiar and compact. Scan the directory, then open a focused editor when you need it.',
  workbench:
    'Built for curation. Tool counts, edit, and save stay beside each set while you choose its tools.',
  guided:
    'One decision at a time. Choose the target, select tools, then review the full selection.',
}
const handoffIds = ['github-search_repositories', 'github-get_pull_request']

// This isolated controller owns synthetic state and never calls the gateway.
// oxlint-disable-next-line eslint/max-lines-per-function
function usePreviewState() {
  const [candidate, setCandidate] = useState<ToolsetCandidate>(initialCandidate)
  const [sets, setSets] = useState(sampleToolsets)
  const [selectedId, setSelectedId] = useState<string | null>(() =>
    initialCandidate() === 'directory' ? null : sampleToolsets[0].id,
  )
  const [memberState, setMemberState] = useState(initialMemberships)
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<ToolsetFilter>('all')
  const [catalogState, setCatalogState] = useState<CatalogState>('ready')
  const [mode, setMode] = useState<DialogMode>(null)
  const [dialogSetId, setDialogSetId] = useState<string | null>(null)
  const [pendingImports, setPendingImports] = useState<string[]>([])
  const [workspaceRevision, setWorkspaceRevision] = useState(0)
  const selected = sets.find((set) => set.id === selectedId) ?? null
  const draftIds = selectedId ? (memberState[selectedId]?.draftIds ?? []) : pendingImports
  const dialogSet = sets.find((set) => set.id === dialogSetId) ?? null

  function openDialog(nextMode: DialogMode, id = selectedId) {
    setDialogSetId(id)
    setMode(nextMode)
  }

  function selectSet(set: McpToolsetView | null) {
    setSelectedId(set?.id ?? null)
    if (!set || pendingImports.length === 0) return
    setMemberState((members) => ({
      ...members,
      [set.id]: {
        ...members[set.id],
        draftIds: [...new Set([...members[set.id].draftIds, ...pendingImports])],
      },
    }))
    setPendingImports([])
    toast.success(`Tools from Servers added to ${set.display_name}. Save to apply the selection.`)
  }

  function changeDraft(update: (ids: string[]) => string[]) {
    if (!selectedId || catalogState !== 'ready') return
    setMemberState((members) => ({
      ...members,
      [selectedId]: { ...members[selectedId], draftIds: update(members[selectedId].draftIds) },
    }))
  }

  function commitMembership(id: string) {
    if (catalogState !== 'ready') return
    setMemberState((members) => ({
      ...members,
      [id]: { ...members[id], savedIds: [...members[id].draftIds] },
    }))
    toast.success(`Saved ${memberState[id].draftIds.length} tools in this preview.`)
  }

  function saveSet(id: string) {
    const membership = memberState[id]
    if (!membership || !membershipIsDirty(membership)) return
    if (membership.savedIds.length > 0 && membership.draftIds.length === 0) {
      openDialog('review', id)
    } else {
      commitMembership(id)
    }
  }

  function saveDetails(details: ToolsetDetails) {
    if (mode === 'create' && sets.some((set) => set.toolset_key === details.toolset_key)) {
      toast.error('That key is already in use. Choose a different key.')
      return false
    }
    const now = new Date().toISOString()
    if (mode === 'create') {
      const set: McpToolsetView = {
        ...details,
        id: crypto.randomUUID(),
        status: 'active',
        created_at: now,
        updated_at: now,
        disabled_at: null,
      }
      setSets((items) => [...items, set])
      setMemberState((members) => ({
        ...members,
        [set.id]: { savedIds: [], draftIds: [...pendingImports] },
      }))
      setSelectedId(set.id)
      setPendingImports([])
      setQuery('')
      setFilter('all')
      setWorkspaceRevision((revision) => revision + 1)
    } else if (dialogSet) {
      setSets((items) =>
        items.map((set) =>
          set.id === dialogSet.id ? { ...set, ...details, updated_at: now } : set,
        ),
      )
    }
    setMode(null)
    toast.success('Tool set details saved in this preview.')
    return true
  }

  function confirmDialog() {
    if (mode === 'disable' && dialogSet) {
      setSets((items) =>
        items.map((set) =>
          set.id === dialogSet.id
            ? { ...set, status: 'disabled', disabled_at: new Date().toISOString() }
            : set,
        ),
      )
      toast.success('Tool set disabled in this preview.')
    }
    if (mode === 'review' && dialogSet) commitMembership(dialogSet.id)
    setMode(null)
  }

  function changeCandidate(value: string) {
    if (!(value in candidateNames)) return
    setCandidate(value as ToolsetCandidate)
    setSelectedId(value === 'directory' ? null : (sets[0]?.id ?? null))
    setMode(null)
    window.history.replaceState(null, '', `?candidate=${value}`)
  }

  function resetSample() {
    setSets(sampleToolsets)
    setMemberState(initialMemberships())
    setQuery('')
    setFilter('all')
    setPendingImports([])
    setCatalogState('ready')
    setSelectedId(candidate === 'directory' ? null : sampleToolsets[0].id)
    setMode(null)
    setWorkspaceRevision((revision) => revision + 1)
  }

  const props: ToolsetCandidateProps = {
    sets: filterToolsets(sets, query, filter),
    allSets: sets,
    query,
    filter,
    onQueryChange: setQuery,
    onFilterChange: setFilter,
    selected,
    onSelect: selectSet,
    draftIds,
    draftSaved: selectedId ? !membershipIsDirty(memberState[selectedId]) : false,
    onToggleTool: (id, checked) =>
      changeDraft((ids) =>
        checked ? [...new Set([...ids, id])] : ids.filter((item) => item !== id),
      ),
    onClearDraft: () => changeDraft(() => []),
    catalogState,
    onRetryCatalog: () => setCatalogState('ready'),
    onCreate: () => openDialog('create', null),
    onEditMetadata: () => openDialog('edit'),
    onDisable: () => openDialog('disable'),
    onReview: () => openDialog('review'),
    onAccess: () => openDialog('access'),
    memberships: Object.fromEntries(
      Object.entries(memberState).map(([id, membership]) => [
        id,
        {
          toolIds: membership.draftIds,
          dirty: membershipIsDirty(membership),
          loading: false,
          error: null,
          saving: false,
        },
      ]),
    ),
    onEditSet: (id) => openDialog('edit', id),
    onSaveSet: saveSet,
    onDisableSet: (id) => openDialog('disable', id),
    workspaceRevision,
  }

  return {
    props,
    candidate,
    changeCandidate,
    resetSample,
    catalogState,
    setCatalogState,
    pendingImports,
    startHandoff: () => {
      setSelectedId(null)
      setPendingImports(handoffIds)
      setQuery('')
      setFilter('all')
      setWorkspaceRevision((revision) => revision + 1)
    },
    dialogProps: {
      mode,
      selected: dialogSet,
      draftIds: dialogSetId ? (memberState[dialogSetId]?.draftIds ?? []) : [],
      onClose: () => setMode(null),
      onSaveDetails: saveDetails,
      onConfirm: confirmDialog,
    },
  }
}

// The linear preview shell keeps its controls together for all three candidate layouts.
// oxlint-disable-next-line eslint/max-lines-per-function
function Preview() {
  const state = usePreviewState()
  const [dark, setDark] = useState(true)
  const { props, candidate } = state
  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark)
  }, [dark])
  return (
    <TooltipProvider>
      <PreviewShell>
        <div className="flex flex-col gap-3 border-b pb-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              <Badge variant="outline">
                {candidate === 'workbench' ? 'Final design' : 'Design reference'}
              </Badge>
              <span className="text-muted-foreground text-xs">Tool Sets / Workbench selected</span>
            </div>
            <div className="flex items-center gap-2">
              <Button variant="ghost" size="sm" onClick={state.resetSample}>
                Reset sample
              </Button>
              <Button
                variant="outline"
                size="icon-sm"
                aria-label={dark ? 'Use light theme' : 'Use dark theme'}
                onClick={() => setDark(!dark)}
              >
                <AppIcon icon={dark ? Sun03Icon : Moon02Icon} aria-hidden />
              </Button>
            </div>
          </div>
          <ToggleGroup
            type="single"
            value={candidate}
            onValueChange={state.changeCandidate}
            spacing={1}
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
        <nav aria-label="MCP sections" className="flex flex-wrap items-center gap-2 border-b pb-4">
          <Button asChild variant="ghost">
            <a href="/designs/index.html?candidate=registry">Servers</a>
          </Button>
          <Button asChild variant="secondary">
            <a href={`?candidate=${candidate}`} aria-current="page">
              Tool Sets
            </a>
          </Button>
          <Button variant="ghost" onClick={props.onAccess}>
            Access
          </Button>
        </nav>
        {state.pendingImports.length > 0 ? (
          <Alert>
            <AlertTitle>{state.pendingImports.length} tools carried from Servers</AlertTitle>
            <AlertDescription>
              Select or create a tool set to add these tools. Its existing selection will be kept.
            </AlertDescription>
          </Alert>
        ) : null}
        {candidate === 'directory' ? (
          <DirectoryCandidate {...props} />
        ) : candidate === 'workbench' ? (
          <WorkbenchCandidate {...props} />
        ) : (
          <GuidedCandidate {...props} />
        )}
        <div className="flex flex-col gap-3 border-t pt-5">
          <div className="flex flex-wrap items-center gap-3">
            <span className="text-muted-foreground text-xs">Preview scenarios</span>
            <Button variant="outline" size="sm" onClick={state.startHandoff}>
              Try server handoff
            </Button>
            <ToggleGroup
              type="single"
              size="sm"
              spacing={1}
              variant="outline"
              value={state.catalogState}
              onValueChange={(value) => {
                if (value) state.setCatalogState(value as CatalogState)
              }}
              aria-label="Catalog scenario"
            >
              <ToggleGroupItem value="ready">Ready</ToggleGroupItem>
              <ToggleGroupItem value="loading">Loading</ToggleGroupItem>
              <ToggleGroupItem value="error">Failed</ToggleGroupItem>
            </ToggleGroup>
          </div>
          <p className="text-muted-foreground text-xs">
            Interactive design preview · Sample data · Changes stay in this page and reset on
            reload.
          </p>
        </div>
        <ToolsetDialogs
          key={`${state.dialogProps.mode}-${state.dialogProps.selected?.id}`}
          {...state.dialogProps}
        />
      </PreviewShell>
      <Toaster theme={dark ? 'dark' : 'light'} />
    </TooltipProvider>
  )
}

createRoot(document.getElementById('root')!).render(<Preview />)
