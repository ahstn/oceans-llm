import { useState } from 'react'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { TooltipProvider } from '@/components/ui/tooltip'
import type { McpServerView, McpToolsetView, McpToolView } from '@/types/api'

const invalidateMock = vi.fn()
const getMcpServerToolsMock = vi.fn()
const getMcpConnectionInfoMock = vi.fn()
const getMcpToolsetToolsMock = vi.fn()
const addMcpToolsetMock = vi.fn()
const saveMcpToolsetMock = vi.fn()
const disableExternalMcpToolsetMock = vi.fn()
const saveMcpToolsetToolsMock = vi.fn()
const toastErrorMock = vi.fn()

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => ({ useLoaderData: vi.fn(), useSearch: vi.fn() }),
  useRouter: () => ({ navigate: vi.fn(), invalidate: invalidateMock }),
}))

vi.mock('sonner', () => ({
  toast: { error: (...args: unknown[]) => toastErrorMock(...args), success: vi.fn() },
}))

vi.mock('@/server/admin-data.functions', () => ({
  addMcpToolset: (...args: unknown[]) => addMcpToolsetMock(...args),
  disableExternalMcpToolset: (...args: unknown[]) => disableExternalMcpToolsetMock(...args),
  getMcpServerTools: (...args: unknown[]) => getMcpServerToolsMock(...args),
  getMcpConnectionInfo: (...args: unknown[]) => getMcpConnectionInfoMock(...args),
  getMcpToolsetTools: (...args: unknown[]) => getMcpToolsetToolsMock(...args),
  saveMcpToolset: (...args: unknown[]) => saveMcpToolsetMock(...args),
  saveMcpToolsetTools: (...args: unknown[]) => saveMcpToolsetToolsMock(...args),
}))

const server: McpServerView = {
  id: 'server_1',
  server_key: 'github',
  display_name: 'GitHub',
  description: null,
  transport: 'streamable_http',
  server_url: 'https://api.githubcopilot.com/mcp/',
  auth_mode: 'none',
  auth_config: {},
  timeout_ms: 30000,
  status: 'active',
  last_discovery_status: 'success',
  last_discovery_at: null,
  last_successful_discovery_at: null,
  last_error_summary: null,
  last_tool_count: 2,
  created_at: '2026-05-27T08:00:00Z',
  updated_at: '2026-05-27T09:00:00Z',
  disabled_at: null,
}

const toolset: McpToolsetView = {
  id: 'toolset_1',
  toolset_key: 'github-readonly',
  display_name: 'GitHub read-only',
  description: 'Repository tools for review.',
  status: 'active',
  created_at: '2026-05-27T08:00:00Z',
  updated_at: '2026-05-27T09:00:00Z',
  disabled_at: null,
}

const secondToolset: McpToolsetView = {
  ...toolset,
  id: 'toolset_2',
  toolset_key: 'docs-bundle',
  display_name: 'Docs bundle',
}

const createdToolset: McpToolsetView = {
  ...toolset,
  id: 'toolset_3',
  toolset_key: 'release-tools',
  display_name: 'Release tools',
}

const firstTool: McpToolView = {
  id: 'tool_1',
  server_id: server.id,
  upstream_name: 'search_repositories',
  display_name: 'Search repositories',
  description: 'Search repositories by name.',
  is_active: true,
  input_schema: { type: 'object', properties: { query: { type: 'string' } } },
  first_discovered_at: '2026-05-27T08:00:00Z',
  last_discovered_at: '2026-05-27T09:00:00Z',
  schema_hash: 'schema-1',
  schema_version: 1,
  deactivated_at: null,
}

const secondTool: McpToolView = {
  ...firstTool,
  id: 'tool_2',
  upstream_name: 'get_pull_request',
  display_name: 'Get pull request',
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((accept) => {
    resolve = accept
  })
  return { promise, resolve }
}

async function renderToolsetsTab({
  initialToolsetId = toolset.id as string | null,
  initialSeedIds = [] as string[],
  initialToolsets = [toolset, secondToolset],
} = {}) {
  const { ToolsetsTab } = await import('@/routes/mcp/-toolsets-tab')
  const onSeedConsumed = vi.fn()

  function ToolsetsHarness() {
    const [selectedToolsetId, setSelectedToolsetId] = useState(initialToolsetId)
    const [seedToolIds, setSeedToolIds] = useState(initialSeedIds)
    const [toolsets, setToolsets] = useState(initialToolsets)
    invalidateMock.mockImplementation(async () => {
      if (addMcpToolsetMock.mock.calls.length > 0) setToolsets([...initialToolsets, createdToolset])
    })

    return (
      <ToolsetsTab
        toolsets={toolsets}
        servers={[server]}
        selectedToolsetId={selectedToolsetId}
        onSelectToolset={setSelectedToolsetId}
        seedToolIds={seedToolIds}
        onSeedConsumed={() => {
          onSeedConsumed()
          setSeedToolIds([])
        }}
      />
    )
  }

  render(<ToolsetsHarness />, { wrapper: TooltipProvider })
  return { onSeedConsumed }
}

function row(set = toolset) {
  return within(screen.getByTestId(`toolset-row-${set.id}`))
}

function saveButton(set = toolset) {
  return screen.getByRole('button', { name: `Save ${set.display_name}` })
}

async function ready() {
  const checkbox = await screen.findByRole('checkbox', { name: firstTool.display_name })
  await waitFor(() => expect(checkbox).toBeEnabled())
  return checkbox
}

describe('ToolsetsTab Workbench', () => {
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', ResizeObserverMock)
    vi.stubGlobal(
      'matchMedia',
      vi.fn().mockImplementation((query: string) => ({
        matches: true,
        media: query,
        onchange: null,
        addListener: vi.fn(),
        removeListener: vi.fn(),
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    )
    vi.clearAllMocks()
    invalidateMock.mockReset()
    getMcpServerToolsMock.mockReset()
    getMcpConnectionInfoMock.mockReset()
    getMcpToolsetToolsMock.mockReset()
    addMcpToolsetMock.mockReset()
    saveMcpToolsetMock.mockReset()
    disableExternalMcpToolsetMock.mockReset()
    saveMcpToolsetToolsMock.mockReset()
    getMcpServerToolsMock.mockResolvedValue({ data: { items: [firstTool, secondTool] } })
    getMcpConnectionInfoMock.mockResolvedValue({
      data: { endpoint: 'https://gateway.example.com/oceans/mcp', client_configurations: [] },
    })
    getMcpToolsetToolsMock.mockImplementation(({ data }: { data: { toolsetId: string } }) =>
      Promise.resolve({
        data: {
          tool_ids:
            data.toolsetId === toolset.id
              ? ['tool_1']
              : data.toolsetId === secondToolset.id
                ? ['tool_2']
                : [],
        },
      }),
    )
    saveMcpToolsetToolsMock.mockImplementation(({ data }: { data: { toolIds: string[] } }) =>
      Promise.resolve({ data: { tool_ids: data.toolIds } }),
    )
    addMcpToolsetMock.mockResolvedValue({ data: { toolset: createdToolset } })
    saveMcpToolsetMock.mockResolvedValue({ data: { toolset } })
    disableExternalMcpToolsetMock.mockResolvedValue({ data: { toolset } })
  })

  it('loads saved tools into the picker and shows real counts beside row actions', async () => {
    await renderToolsetsTab()
    expect(await ready()).toBeChecked()
    expect(screen.getByRole('checkbox', { name: secondTool.display_name })).not.toBeChecked()
    expect(row().getByText('1 tool')).toBeInTheDocument()
    expect(row(secondToolset).getByText('1 tool')).toBeInTheDocument()
    expect(row().queryByText(toolset.toolset_key)).not.toBeInTheDocument()
    expect(row().getByRole('button', { name: `Edit ${toolset.display_name}` })).toBeEnabled()
    expect(saveButton()).toBeDisabled()
    expect(getMcpToolsetToolsMock).toHaveBeenCalledWith({ data: { toolsetId: toolset.id } })
    expect(getMcpToolsetToolsMock).toHaveBeenCalledWith({ data: { toolsetId: secondToolset.id } })
  })

  it('selects the first tool set when the route has no selection or carried tools', async () => {
    await renderToolsetsTab({ initialToolsetId: null })

    expect(await ready()).toBeChecked()
    expect(screen.getByRole('radio', { name: `Select ${toolset.display_name}` })).toBeChecked()
  })

  it('loads gateway connection info on demand and preserves unsaved tools', async () => {
    await renderToolsetsTab()
    await ready()
    fireEvent.click(screen.getByRole('checkbox', { name: secondTool.display_name }))
    expect(saveButton()).toBeEnabled()
    expect(getMcpConnectionInfoMock).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Connection Info', exact: true }))
    const dialog = await screen.findByRole('dialog', { name: 'Connection Info', exact: true })
    await waitFor(() => expect(dialog).toHaveTextContent('https://gateway.example.com/oceans/mcp'))
    expect(getMcpConnectionInfoMock).toHaveBeenCalledExactlyOnceWith()
    expect(dialog).toHaveTextContent(toolset.display_name)
    expect(dialog).toHaveTextContent('The client can use all tools granted to that key.')

    fireEvent.click(within(dialog).getByRole('button', { name: 'Close' }))
    expect(screen.getByRole('checkbox', { name: secondTool.display_name })).toBeChecked()
    expect(row().getByRole('status')).toHaveTextContent('2 tools · Unsaved')
    expect(saveButton()).toBeEnabled()
    expect(saveMcpToolsetToolsMock).not.toHaveBeenCalled()
  })

  it('falls back to the first tool set when a stale link selects a missing set', async () => {
    await renderToolsetsTab({ initialToolsetId: 'missing_toolset' })

    expect(await ready()).toBeChecked()
    expect(screen.getByRole('radio', { name: `Select ${toolset.display_name}` })).toBeChecked()
    expect(saveButton()).toBeDisabled()
  })

  it('saves the draft and returns its navigator status to saved', async () => {
    await renderToolsetsTab()
    await ready()
    fireEvent.click(screen.getByRole('checkbox', { name: secondTool.display_name }))
    expect(row().getByText('2 tools')).toBeInTheDocument()
    expect(saveButton()).toBeEnabled()
    fireEvent.click(saveButton())
    await waitFor(() =>
      expect(saveMcpToolsetToolsMock).toHaveBeenCalledWith({
        data: { toolsetId: toolset.id, toolIds: ['tool_1', 'tool_2'] },
      }),
    )
    await waitFor(() => expect(saveButton()).toBeDisabled())
    expect(screen.getByRole('checkbox', { name: secondTool.display_name })).toBeChecked()
  })

  it('disables save when edits return to the saved selection', async () => {
    await renderToolsetsTab()
    await ready()
    const secondCheckbox = screen.getByRole('checkbox', { name: secondTool.display_name })
    fireEvent.click(secondCheckbox)
    expect(saveButton()).toBeEnabled()
    fireEvent.click(secondCheckbox)
    expect(saveButton()).toBeDisabled()
    expect(row().getByText('1 tool')).toBeInTheDocument()
    expect(saveMcpToolsetToolsMock).not.toHaveBeenCalled()
  })

  it('keeps the current draft and its save action visible when the navigator filter hides it', async () => {
    await renderToolsetsTab()
    await ready()
    fireEvent.click(screen.getByRole('checkbox', { name: secondTool.display_name }))
    fireEvent.change(screen.getByRole('textbox', { name: 'Search tool sets' }), {
      target: { value: 'no matching collection' },
    })

    expect(screen.getByText('Current set · outside this filter')).toBeVisible()
    expect(screen.getByText('No matching tool sets')).toBeVisible()
    expect(row().getByText('2 tools')).toBeVisible()
    expect(screen.getByRole('radio', { name: `Select ${toolset.display_name}` })).toBeChecked()
    expect(saveButton()).toBeVisible()
    expect(saveButton()).toBeEnabled()
    fireEvent.click(saveButton())

    await waitFor(() =>
      expect(saveMcpToolsetToolsMock).toHaveBeenCalledWith({
        data: { toolsetId: toolset.id, toolIds: ['tool_1', 'tool_2'] },
      }),
    )
    await waitFor(() => expect(saveButton()).toBeDisabled())
  })

  it('retains edits after save fails and lets the same draft retry', async () => {
    saveMcpToolsetToolsMock.mockRejectedValueOnce(new Error('Membership update failed'))
    await renderToolsetsTab()
    await ready()
    fireEvent.click(screen.getByRole('checkbox', { name: secondTool.display_name }))
    fireEvent.click(saveButton())
    await waitFor(() => expect(toastErrorMock).toHaveBeenCalledWith('Membership update failed'))
    await waitFor(() => expect(saveButton()).toBeEnabled())
    expect(screen.getByRole('checkbox', { name: secondTool.display_name })).toBeChecked()
    expect(row().getByText('2 tools')).toBeInTheDocument()
    fireEvent.click(saveButton())
    await waitFor(() => expect(saveMcpToolsetToolsMock).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(saveButton()).toBeDisabled())
  })

  it('preserves drafts across navigation and saves the row that was clicked', async () => {
    const pendingSave = deferred<{ data: { tool_ids: string[] } }>()
    saveMcpToolsetToolsMock.mockReturnValueOnce(pendingSave.promise)
    await renderToolsetsTab()
    await ready()
    fireEvent.click(screen.getByRole('checkbox', { name: secondTool.display_name }))
    fireEvent.click(screen.getByRole('radio', { name: `Select ${secondToolset.display_name}` }))
    expect(screen.getByRole('checkbox', { name: firstTool.display_name })).not.toBeChecked()
    expect(screen.getByRole('checkbox', { name: secondTool.display_name })).toBeChecked()
    expect(row().getByText('2 tools')).toBeInTheDocument()
    fireEvent.click(saveButton())
    await waitFor(() =>
      expect(saveMcpToolsetToolsMock).toHaveBeenCalledWith({
        data: { toolsetId: toolset.id, toolIds: ['tool_1', 'tool_2'] },
      }),
    )
    await act(async () => pendingSave.resolve({ data: { tool_ids: ['tool_1', 'tool_2'] } }))
    expect(
      screen.getByRole('radio', { name: `Select ${secondToolset.display_name}` }),
    ).toBeChecked()
    expect(screen.getByRole('checkbox', { name: firstTool.display_name })).not.toBeChecked()
    fireEvent.click(screen.getByRole('radio', { name: `Select ${toolset.display_name}` }))
    expect(screen.getByRole('checkbox', { name: firstTool.display_name })).toBeChecked()
    expect(screen.getByRole('checkbox', { name: secondTool.display_name })).toBeChecked()
    expect(saveButton()).toBeDisabled()
  })

  it('does not turn a failed membership read into an empty saved set', async () => {
    getMcpToolsetToolsMock.mockRejectedValueOnce(new Error('Saved tools are unavailable'))
    await renderToolsetsTab()
    const retry = await screen.findByRole('button', {
      name: `Retry tools for ${toolset.display_name}`,
    })
    expect(row().queryByText('0 tools')).not.toBeInTheDocument()
    expect(saveButton()).toBeDisabled()
    fireEvent.click(retry)
    expect(await ready()).toBeChecked()
    expect(row().getByText('1 tool')).toBeInTheDocument()
    expect(saveButton()).toBeDisabled()
  })

  it('merges carried tools after saved membership loads without losing the saved selection', async () => {
    const pendingRead = deferred<{ data: { tool_ids: string[] } }>()
    getMcpToolsetToolsMock.mockReturnValueOnce(pendingRead.promise)
    const { onSeedConsumed } = await renderToolsetsTab({ initialSeedIds: ['tool_2'] })
    expect(saveButton()).toBeDisabled()
    await act(async () => pendingRead.resolve({ data: { tool_ids: ['tool_1'] } }))
    expect(await ready()).toBeChecked()
    expect(screen.getByRole('checkbox', { name: secondTool.display_name })).toBeChecked()
    expect(onSeedConsumed).toHaveBeenCalledTimes(1)
    expect(row().getByText('2 tools')).toBeInTheDocument()
    fireEvent.click(saveButton())
    await waitFor(() =>
      expect(saveMcpToolsetToolsMock).toHaveBeenCalledWith({
        data: { toolsetId: toolset.id, toolIds: ['tool_1', 'tool_2'] },
      }),
    )
  })

  it('keeps carried tools unassigned until the first tool set is chosen', async () => {
    await renderToolsetsTab({ initialToolsetId: null, initialSeedIds: ['tool_1'] })
    expect(screen.getByText('Choose a tool set')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('radio', { name: `Select ${secondToolset.display_name}` }))
    expect(await ready()).toBeChecked()
    expect(screen.getByRole('checkbox', { name: secondTool.display_name })).toBeChecked()
    expect(saveButton(secondToolset)).toBeEnabled()
    expect(saveButton()).toBeDisabled()
  })

  it('keeps carried tools assignable when a stale link selects a missing set', async () => {
    await renderToolsetsTab({
      initialToolsetId: 'missing_toolset',
      initialSeedIds: ['tool_1'],
    })
    expect(screen.getByText('Choose a tool set')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('radio', { name: `Select ${secondToolset.display_name}` }))

    expect(await ready()).toBeChecked()
    expect(screen.getByRole('checkbox', { name: secondTool.display_name })).toBeChecked()
    expect(saveButton(secondToolset)).toBeEnabled()
    fireEvent.click(saveButton(secondToolset))
    await waitFor(() =>
      expect(saveMcpToolsetToolsMock).toHaveBeenCalledWith({
        data: { toolsetId: secondToolset.id, toolIds: ['tool_2', 'tool_1'] },
      }),
    )
  })

  it('assigns carried tools to a newly created set', async () => {
    await renderToolsetsTab({ initialToolsetId: null, initialSeedIds: ['tool_1'] })
    fireEvent.click(screen.getByRole('button', { name: 'New tool set' }))
    fireEvent.change(screen.getByLabelText('Key'), {
      target: { value: createdToolset.toolset_key },
    })
    fireEvent.change(screen.getByLabelText('Display name'), {
      target: { value: createdToolset.display_name },
    })
    fireEvent.click(screen.getByRole('button', { name: /Create tool ?set/ }))
    await waitFor(() => expect(saveButton(createdToolset)).toBeEnabled())
    expect(screen.getByRole('checkbox', { name: firstTool.display_name })).toBeChecked()
    fireEvent.click(saveButton(createdToolset))
    await waitFor(() =>
      expect(saveMcpToolsetToolsMock).toHaveBeenCalledWith({
        data: { toolsetId: createdToolset.id, toolIds: ['tool_1'] },
      }),
    )
  })

  it('retains a newly created draft until its tool set reaches the loader data', async () => {
    const { ToolsetsTab } = await import('@/routes/mcp/-toolsets-tab')
    invalidateMock.mockResolvedValue(undefined)
    function DelayedLoader({ toolsets }: { toolsets: McpToolsetView[] }) {
      const [selectedToolsetId, onSelectToolset] = useState<string | null>(null)
      const [seedToolIds, setSeedToolIds] = useState(['tool_1'])
      return (
        <ToolsetsTab
          toolsets={toolsets}
          servers={[server]}
          selectedToolsetId={selectedToolsetId}
          onSelectToolset={onSelectToolset}
          seedToolIds={seedToolIds}
          onSeedConsumed={() => setSeedToolIds([])}
        />
      )
    }
    const { rerender } = render(<DelayedLoader toolsets={[toolset]} />, {
      wrapper: TooltipProvider,
    })
    fireEvent.click(screen.getByRole('button', { name: 'New tool set' }))
    fireEvent.change(screen.getByLabelText('Key'), {
      target: { value: createdToolset.toolset_key },
    })
    fireEvent.change(screen.getByLabelText('Display name'), {
      target: { value: createdToolset.display_name },
    })
    fireEvent.click(screen.getByRole('button', { name: /Create tool ?set/ }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(screen.getByRole('radio', { name: `Select ${toolset.display_name}` })).not.toBeChecked()

    rerender(<DelayedLoader toolsets={[toolset, createdToolset]} />)

    expect(await ready()).toBeChecked()
    expect(
      screen.getByRole('radio', { name: `Select ${createdToolset.display_name}` }),
    ).toBeChecked()
    expect(saveButton(createdToolset)).toBeEnabled()
    fireEvent.click(saveButton(createdToolset))
    await waitFor(() =>
      expect(saveMcpToolsetToolsMock).toHaveBeenCalledWith({
        data: { toolsetId: createdToolset.id, toolIds: ['tool_1'] },
      }),
    )
  })

  it('prevents saving a draft until the tool catalog loads', async () => {
    const pendingCatalog = deferred<{ data: { items: McpToolView[] } }>()
    getMcpServerToolsMock.mockReturnValueOnce(pendingCatalog.promise)
    await renderToolsetsTab({ initialSeedIds: ['tool_2'] })
    await waitFor(() => expect(row().getByText('2 tools')).toBeInTheDocument())
    expect(saveButton()).toBeDisabled()
    fireEvent.click(saveButton())
    expect(saveMcpToolsetToolsMock).not.toHaveBeenCalled()
    await act(async () => pendingCatalog.resolve({ data: { items: [firstTool, secondTool] } }))
    await waitFor(() => expect(saveButton()).toBeEnabled())
  })

  it('retains a draft after catalog failure and enables save after retry', async () => {
    getMcpServerToolsMock.mockRejectedValueOnce(new Error('Discovery catalog is unavailable'))
    await renderToolsetsTab({ initialSeedIds: ['tool_2'] })
    const retry = await screen.findByRole('button', { name: 'Retry catalog' })
    expect(saveButton()).toBeDisabled()
    fireEvent.click(retry)
    await waitFor(() => expect(saveButton()).toBeEnabled())
    expect(screen.getByRole('checkbox', { name: firstTool.display_name })).toBeChecked()
    expect(screen.getByRole('checkbox', { name: secondTool.display_name })).toBeChecked()
    expect(row().getByText('2 tools')).toBeInTheDocument()
  })

  it('requires explicit removal of unavailable saved IDs before saving edits', async () => {
    getMcpToolsetToolsMock.mockResolvedValueOnce({ data: { tool_ids: ['tool_1', 'missing_tool'] } })
    await renderToolsetsTab()
    await ready()
    fireEvent.click(screen.getByRole('checkbox', { name: secondTool.display_name }))
    expect(row().getByText('3 tools')).toBeInTheDocument()
    expect(saveButton()).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'Remove missing_tool' }))
    expect(saveButton()).toBeEnabled()
    fireEvent.click(saveButton())
    await waitFor(() =>
      expect(saveMcpToolsetToolsMock).toHaveBeenCalledWith({
        data: { toolsetId: toolset.id, toolIds: ['tool_1', 'tool_2'] },
      }),
    )
  })

  it('confirms removal of the final saved tool before replacing membership', async () => {
    await renderToolsetsTab()
    fireEvent.click(await ready())
    fireEvent.click(saveButton())
    const confirmation = screen.getByRole('alertdialog')
    expect(saveMcpToolsetToolsMock).not.toHaveBeenCalled()
    fireEvent.click(within(confirmation).getByRole('button', { name: 'Cancel' }))
    expect(saveMcpToolsetToolsMock).not.toHaveBeenCalled()
    expect(row().getByText('0 tools')).toBeInTheDocument()
    fireEvent.click(saveButton())
    fireEvent.click(
      within(screen.getByRole('alertdialog')).getByRole('button', {
        name: /Remove all tools|Save empty/,
      }),
    )
    await waitFor(() =>
      expect(saveMcpToolsetToolsMock).toHaveBeenCalledWith({
        data: { toolsetId: toolset.id, toolIds: [] },
      }),
    )
  })

  it('edits the chosen navigator row without changing the selected tool set', async () => {
    await renderToolsetsTab()
    await ready()
    fireEvent.click(screen.getByRole('button', { name: `Edit ${secondToolset.display_name}` }))
    const dialog = screen.getByRole('dialog')
    expect(within(dialog).getByLabelText('Display name')).toHaveValue(secondToolset.display_name)
    fireEvent.change(within(dialog).getByLabelText('Display name'), {
      target: { value: 'Docs and guides' },
    })
    fireEvent.change(within(dialog).getByLabelText('Description'), {
      target: { value: 'Shared reference tools.' },
    })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save details' }))
    await waitFor(() =>
      expect(saveMcpToolsetMock).toHaveBeenCalledWith({
        data: {
          toolsetId: secondToolset.id,
          input: { display_name: 'Docs and guides', description: 'Shared reference tools.' },
        },
      }),
    )
    expect(screen.getByRole('radio', { name: `Select ${toolset.display_name}` })).toBeChecked()
  })

  it('disables the selected tool set through the server function', async () => {
    await renderToolsetsTab()
    await ready()
    fireEvent.click(screen.getByRole('button', { name: `Disable ${toolset.display_name}` }))
    await waitFor(() =>
      expect(disableExternalMcpToolsetMock).toHaveBeenCalledWith({
        data: { toolsetId: toolset.id },
      }),
    )
  })
})
