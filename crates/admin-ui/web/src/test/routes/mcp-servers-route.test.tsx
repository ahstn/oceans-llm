import { useState } from 'react'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { McpServerView, McpToolView, RecommendedMcpServerView } from '@/types/api'

const navigateMock = vi.fn()
const invalidateMock = vi.fn()
const getMcpServerToolsMock = vi.fn()
const getMcpCredentialBindingsMock = vi.fn()
const addMcpServerMock = vi.fn()
const saveMcpServerMock = vi.fn()
const disableExternalMcpServerMock = vi.fn()
const refreshExternalMcpServerDiscoveryMock = vi.fn()

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => ({ useLoaderData: vi.fn(), useSearch: vi.fn() }),
  useRouter: () => ({
    navigate: navigateMock,
    invalidate: invalidateMock,
  }),
}))

vi.mock('sonner', () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
  },
}))

vi.mock('@/server/admin-data.functions', () => ({
  addMcpServer: (...args: unknown[]) => addMcpServerMock(...args),
  disableExternalMcpServer: (...args: unknown[]) => disableExternalMcpServerMock(...args),
  getMcpCredentialBindings: (...args: unknown[]) => getMcpCredentialBindingsMock(...args),
  getMcpServerTools: (...args: unknown[]) => getMcpServerToolsMock(...args),
  refreshExternalMcpServerDiscovery: (...args: unknown[]) =>
    refreshExternalMcpServerDiscoveryMock(...args),
  removeMcpCredentialBinding: vi.fn(),
  saveMcpCredentialBinding: vi.fn(),
  saveMcpServer: (...args: unknown[]) => saveMcpServerMock(...args),
}))

const server: McpServerView = {
  id: 'server_1',
  server_key: 'github',
  display_name: 'GitHub',
  description: 'GitHub MCP',
  transport: 'streamable_http',
  server_url: 'https://api.githubcopilot.com/mcp/',
  auth_mode: 'gateway_bearer_token',
  auth_config: { secret_ref: 'env/OCEANS_MCP_DISCOVERY_GITHUB_TOKEN' },
  timeout_ms: 30000,
  status: 'active',
  last_discovery_status: 'success',
  last_discovery_at: '2026-05-27T09:00:00Z',
  last_successful_discovery_at: '2026-05-27T09:00:00Z',
  last_error_summary: null,
  last_tool_count: 1,
  created_at: '2026-05-27T08:00:00Z',
  updated_at: '2026-05-27T09:00:00Z',
  disabled_at: null,
}

const tool: McpToolView = {
  id: 'tool_1',
  server_id: 'server_1',
  upstream_name: 'create_issue',
  display_name: 'create_issue',
  description: 'Create issue',
  input_schema: {},
  schema_hash: 'sha256:abc123',
  schema_version: 2,
  is_active: false,
  first_discovered_at: '2026-05-27T08:15:00Z',
  last_discovered_at: '2026-05-27T09:00:00Z',
  deactivated_at: '2026-05-27T10:00:00Z',
}

const recommended: RecommendedMcpServerView = {
  catalog_key: 'linear',
  display_name: 'Linear',
  description: 'Linear MCP',
  transport: 'streamable_http',
  server_url: 'https://mcp.linear.app/mcp',
  auth_mode: 'none',
  auth_config: {},
  documentation_url: null,
  tags: ['tickets'],
}

async function renderServersTab(initialSelectedServerId: string | null = null) {
  const { ServersTab } = await import('@/routes/mcp/-servers-tab')
  function ServersTabHarness() {
    const [selectedServerId, setSelectedServerId] = useState<string | null>(initialSelectedServerId)
    return (
      <ServersTab
        servers={[server]}
        recommended={[recommended]}
        selectedServerId={selectedServerId}
        onSelectServer={setSelectedServerId}
        onAddToToolset={vi.fn()}
      />
    )
  }

  render(<ServersTabHarness />)
}

describe('ServersTab', () => {
  afterEach(() => {
    cleanup()
  })

  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', ResizeObserverMock)
    // Force the inline (wide) master-detail layout so the detail renders in-grid.
    window.matchMedia = vi.fn().mockImplementation((query: string) => ({
      matches: true,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }))

    getMcpServerToolsMock.mockReset()
    getMcpCredentialBindingsMock.mockReset()
    addMcpServerMock.mockReset()
    saveMcpServerMock.mockReset()
    disableExternalMcpServerMock.mockReset()
    refreshExternalMcpServerDiscoveryMock.mockReset()
    navigateMock.mockReset()
    invalidateMock.mockReset()

    getMcpServerToolsMock.mockResolvedValue({ data: { items: [tool] } })
    getMcpCredentialBindingsMock.mockResolvedValue({ data: { items: [] } })
    addMcpServerMock.mockResolvedValue({ data: { server } })
    saveMcpServerMock.mockResolvedValue({ data: { server } })
    disableExternalMcpServerMock.mockResolvedValue({
      data: { server: { ...server, status: 'disabled' } },
    })
    refreshExternalMcpServerDiscoveryMock.mockResolvedValue({
      data: { server, status: 'success', error_summary: null, tools: [tool] },
    })
  })

  it('renders server diagnostics and discovered tools', async () => {
    await renderServersTab()

    expect(screen.getByText(/registered/)).toBeInTheDocument()
    expect(screen.getByTestId('mcp-server-list')).toBeInTheDocument()
    expect(screen.getByText('https://api.githubcopilot.com/mcp/')).toBeInTheDocument()
    expect(screen.getByText('gateway bearer token')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Open GitHub' }))
    expect(screen.getByText('/mcp/github')).toBeInTheDocument()

    fireEvent.click(screen.getAllByRole('button', { name: 'Tools' })[0])

    await waitFor(() => expect(screen.getByText('sha256:abc123')).toBeInTheDocument())
    expect(screen.getByText('Inactive')).toBeInTheDocument()
    const toolRow = screen
      .getAllByRole('row')
      .find((row) => within(row).queryByText('sha256:abc123') !== null)
    expect(toolRow).toBeDefined()
    expect(within(toolRow as HTMLTableRowElement).getByText('2')).toBeInTheDocument()
  })

  it('refreshes discovery and renders refresh feedback', async () => {
    await renderServersTab('server_1')
    fireEvent.click(screen.getByRole('button', { name: 'Refresh GitHub' }))

    await waitFor(() => {
      expect(refreshExternalMcpServerDiscoveryMock).toHaveBeenCalledWith({
        data: { serverId: 'server_1' },
      })
    })
    await waitFor(() => expect(screen.getByText('Discovery success')).toBeInTheDocument())
  })

  it('renders refresh response errors without waiting for loader data', async () => {
    refreshExternalMcpServerDiscoveryMock.mockResolvedValueOnce({
      data: {
        server: { ...server, last_error_summary: 'old upstream failure' },
        status: 'failed',
        error_summary: 'new upstream failure',
        tools: [],
      },
    })
    await renderServersTab('server_1')
    fireEvent.click(screen.getByRole('button', { name: 'Refresh GitHub' }))

    await waitFor(() => expect(screen.getByText('Discovery failed')).toBeInTheDocument())
    expect(screen.getByText('new upstream failure')).toBeInTheDocument()
    expect(screen.queryByText('old upstream failure')).not.toBeInTheDocument()
  })

  it('imports recommended servers through the server function', async () => {
    await renderServersTab()
    fireEvent.click(screen.getByRole('button', { name: 'Import' }))

    await waitFor(() => {
      expect(addMcpServerMock).toHaveBeenCalledWith({
        data: { recommended_catalog_key: 'linear' },
      })
    })
  })

  it('submits custom add and edit flows', async () => {
    await renderServersTab()
    fireEvent.click(screen.getByRole('button', { name: 'Add server' }))
    fireEvent.change(screen.getByLabelText('Server key'), { target: { value: 'slack' } })
    fireEvent.change(screen.getByLabelText('Display name'), { target: { value: 'Slack' } })
    fireEvent.change(screen.getByLabelText('Server URL'), {
      target: { value: 'https://mcp.slack.com/mcp' },
    })
    const addButtons = screen.getAllByRole('button', { name: 'Add server' })
    fireEvent.click(addButtons[addButtons.length - 1])

    await waitFor(() => {
      expect(addMcpServerMock).toHaveBeenCalledWith({
        data: expect.objectContaining({
          server_key: 'slack',
          display_name: 'Slack',
          server_url: 'https://mcp.slack.com/mcp',
        }),
      })
    })

    fireEvent.click(screen.getByRole('button', { name: 'Edit GitHub' }))
    fireEvent.change(screen.getByLabelText('Display name'), { target: { value: 'GitHub MCP' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }))

    await waitFor(() => {
      expect(saveMcpServerMock).toHaveBeenCalledWith({
        data: {
          serverId: 'server_1',
          input: expect.objectContaining({ display_name: 'GitHub MCP' }),
        },
      })
    })
  })

  it('disables active servers through the server function', async () => {
    await renderServersTab()
    fireEvent.click(screen.getByRole('button', { name: 'Delete GitHub' }))

    await waitFor(() => {
      expect(disableExternalMcpServerMock).toHaveBeenCalledWith({
        data: { serverId: 'server_1' },
      })
    })
  })
})
