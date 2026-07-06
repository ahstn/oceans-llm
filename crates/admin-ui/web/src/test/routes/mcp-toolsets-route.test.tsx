import { useState } from 'react'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { McpServerView, McpToolsetView } from '@/types/api'

const invalidateMock = vi.fn()
const getMcpServerToolsMock = vi.fn()
const addMcpToolsetMock = vi.fn()
const saveMcpToolsetMock = vi.fn()
const disableExternalMcpToolsetMock = vi.fn()
const saveMcpToolsetToolsMock = vi.fn()

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => ({ useLoaderData: vi.fn(), useSearch: vi.fn() }),
  useRouter: () => ({ navigate: vi.fn(), invalidate: invalidateMock }),
}))

vi.mock('sonner', () => ({ toast: { error: vi.fn(), success: vi.fn() } }))

vi.mock('@/server/admin-data.functions', () => ({
  addMcpToolset: (...args: unknown[]) => addMcpToolsetMock(...args),
  disableExternalMcpToolset: (...args: unknown[]) => disableExternalMcpToolsetMock(...args),
  getMcpServerTools: (...args: unknown[]) => getMcpServerToolsMock(...args),
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
  last_tool_count: 1,
  created_at: '2026-05-27T08:00:00Z',
  updated_at: '2026-05-27T09:00:00Z',
  disabled_at: null,
}

const toolset: McpToolsetView = {
  id: 'toolset_1',
  toolset_key: 'github-readonly',
  display_name: 'GitHub read-only',
  description: null,
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

async function renderToolsetsTab(seedToolIds: string[] = []) {
  const { ToolsetsTab } = await import('@/routes/mcp/-toolsets-tab')
  render(
    <ToolsetsTab
      toolsets={[toolset]}
      servers={[server]}
      selectedToolsetId="toolset_1"
      onSelectToolset={vi.fn()}
      seedToolIds={seedToolIds}
      onSeedConsumed={vi.fn()}
    />,
  )
}

describe('ToolsetsTab', () => {
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', ResizeObserverMock)
    vi.stubGlobal('matchMedia', vi.fn().mockImplementation((query: string) => ({
      matches: true,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })))

    invalidateMock.mockReset()
    getMcpServerToolsMock.mockReset()
    addMcpToolsetMock.mockReset()
    saveMcpToolsetMock.mockReset()
    disableExternalMcpToolsetMock.mockReset()
    saveMcpToolsetToolsMock.mockReset()

    getMcpServerToolsMock.mockResolvedValue({ data: { items: [] } })
    saveMcpToolsetToolsMock.mockResolvedValue({ data: { tool_ids: ['tool_1'] } })
  })

  it('surfaces the write-only membership constraint honestly', async () => {
    await renderToolsetsTab()
    expect(screen.getByText('Membership is write-only')).toBeInTheDocument()
  })

  it('carries a Servers-tab selection into the editor and replaces membership', async () => {
    await renderToolsetsTab(['tool_1'])

    expect(screen.getByText('1 tools carried over')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Replace membership' }))

    await waitFor(() => {
      expect(saveMcpToolsetToolsMock).toHaveBeenCalledWith({
        data: { toolsetId: 'toolset_1', toolIds: ['tool_1'] },
      })
    })
  })

  it('clears staged membership when switching toolsets', async () => {
    const { ToolsetsTab } = await import('@/routes/mcp/-toolsets-tab')

    function ToolsetsHarness() {
      const [selectedToolsetId, setSelectedToolsetId] = useState('toolset_1')
      return (
        <ToolsetsTab
          toolsets={[toolset, secondToolset]}
          servers={[server]}
          selectedToolsetId={selectedToolsetId}
          onSelectToolset={setSelectedToolsetId}
          seedToolIds={['tool_1']}
          onSeedConsumed={vi.fn()}
        />
      )
    }

    render(<ToolsetsHarness />)

    fireEvent.click(screen.getByRole('button', { name: /Docs bundle/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Replace membership' }))

    await waitFor(() => {
      expect(saveMcpToolsetToolsMock).toHaveBeenCalledWith({
        data: { toolsetId: 'toolset_2', toolIds: [] },
      })
    })
  })

  it('disables a toolset through the server function', async () => {
    await renderToolsetsTab()
    fireEvent.click(screen.getByRole('button', { name: 'Disable' }))

    await waitFor(() => {
      expect(disableExternalMcpToolsetMock).toHaveBeenCalledWith({
        data: { toolsetId: 'toolset_1' },
      })
    })
  })
})
