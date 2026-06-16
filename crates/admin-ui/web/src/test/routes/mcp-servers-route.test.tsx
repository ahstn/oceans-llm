import { useState } from 'react'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
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

const activeTool: McpToolView = {
  ...tool,
  id: 'tool_2',
  upstream_name: 'query_docs',
  display_name: 'query_docs',
  description:
    'Retrieves and queries up-to-date documentation and code examples from Context7 for any programming library.',
  input_schema: {
    type: 'object',
    properties: {
      query: {
        type: 'string',
        description:
          'The question or task you need help with. Be specific and include relevant details.',
      },
    },
    required: ['query'],
  },
  schema_hash: 'sha256:def456',
  schema_version: 3,
  is_active: true,
  deactivated_at: null,
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
  const onAddToToolset = vi.fn()

  function ServersTabHarness() {
    const [selectedServerId, setSelectedServerId] = useState<string | null>(initialSelectedServerId)
    return (
      <ServersTab
        servers={[server]}
        recommended={[recommended]}
        selectedServerId={selectedServerId}
        onSelectServer={setSelectedServerId}
        onAddToToolset={onAddToToolset}
      />
    )
  }

  render(<ServersTabHarness />)

  return { onAddToToolset }
}

describe('ServersTab', () => {
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', ResizeObserverMock)
    // Force the inline (wide) master-detail layout so the detail renders in-grid.
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

    await waitFor(() => expect(screen.getByText('Create issue')).toBeInTheDocument())
    expect(screen.getByText('Inactive')).toBeInTheDocument()
    expect(screen.queryByText('sha256:abc123')).not.toBeInTheDocument()
    expect(screen.getByRole('checkbox', { name: 'Select create_issue' })).toBeDisabled()

    fireEvent.click(screen.getByRole('button', { name: 'Show create_issue schema' }))

    expect(screen.queryByText('sha256:abc123')).not.toBeInTheDocument()
    expect(screen.getByText('Upstream name')).toBeInTheDocument()
    expect(screen.getByText('2')).toBeInTheDocument()
    expect(screen.getByText('{}')).toBeInTheDocument()
  })

  it('keeps selected tool actions visible without hiding the tool rows', async () => {
    getMcpServerToolsMock.mockResolvedValueOnce({ data: { items: [activeTool, tool] } })
    const { onAddToToolset } = await renderServersTab()

    fireEvent.click(screen.getByRole('button', { name: 'Open GitHub' }))
    fireEvent.click(screen.getAllByRole('button', { name: 'Tools' })[0])

    await waitFor(() => expect(screen.getByText('query_docs')).toBeInTheDocument())

    fireEvent.click(screen.getByRole('checkbox', { name: 'Select query_docs' }))

    expect(screen.getByText('1 tool selected')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Clear' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Add to toolset' })).toBeInTheDocument()
    expect(screen.getByText('query_docs')).toBeInTheDocument()
    expect(screen.getByText('Create issue')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Add to toolset' }))

    expect(onAddToToolset).toHaveBeenCalledTimes(1)
    expect(onAddToToolset).toHaveBeenCalledWith(['tool_2'])
  })

  it('contains expanded JSON schema overflow inside the tools panel', async () => {
    getMcpServerToolsMock.mockResolvedValueOnce({ data: { items: [activeTool] } })
    await renderServersTab()

    fireEvent.click(screen.getByRole('button', { name: 'Open GitHub' }))
    fireEvent.click(screen.getAllByRole('button', { name: 'Tools' })[0])

    await waitFor(() => expect(screen.getByText('query_docs')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: 'Show query_docs schema' }))

    expect(screen.getByTestId('mcp-server-tools')).toHaveClass(
      'min-w-0',
      'max-w-full',
      'overflow-hidden',
    )
    expect(screen.getByTestId('mcp-tool-schema-scroll')).toHaveClass(
      'min-w-0',
      'max-w-full',
      'overflow-hidden',
    )
    expect(screen.getByTestId('mcp-tool-schema-code')).toHaveClass(
      'max-w-full',
      'overflow-x-auto',
      'overflow-y-auto',
    )
    expect(screen.getByText('Tool ID')).toBeInTheDocument()
    expect(screen.getByText('Upstream name')).toBeInTheDocument()
    expect(screen.getByText('Version')).toBeInTheDocument()
    expect(screen.getByText('JSON schema')).toBeInTheDocument()
    expect(screen.queryByText('sha256:def456')).not.toBeInTheDocument()
    expect(screen.queryByText('First seen')).not.toBeInTheDocument()
    expect(screen.queryByText('Last seen')).not.toBeInTheDocument()
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
    fireEvent.click(screen.getByRole('button', { name: 'Disable GitHub' }))

    await waitFor(() => {
      expect(disableExternalMcpServerMock).toHaveBeenCalledWith({
        data: { serverId: 'server_1' },
      })
    })
  })
})
