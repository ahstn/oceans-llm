import { useState } from 'react'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { TooltipProvider } from '@/components/ui/tooltip'
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

async function renderServersTab(
  initialSelectedServerId: string | null = null,
  servers: McpServerView[] = [server],
) {
  const { ServersTab } = await import('@/routes/mcp/-servers-tab')
  const onAddToToolset = vi.fn()

  function ServersTabHarness() {
    const [selectedServerId, setSelectedServerId] = useState<string | null>(initialSelectedServerId)
    return (
      <ServersTab
        servers={servers}
        recommended={[recommended]}
        selectedServerId={selectedServerId}
        onSelectServer={setSelectedServerId}
        onAddToToolset={onAddToToolset}
      />
    )
  }

  render(<ServersTabHarness />, { wrapper: TooltipProvider })

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

    expect(screen.getByText('active registrations')).toBeInTheDocument()
    const registry = within(screen.getByTestId('mcp-server-list'))
    expect(registry.getByText('api.githubcopilot.com')).toHaveAttribute('title', server.server_url)
    expect(registry.getByText('Gateway bearer token')).toBeInTheDocument()
    expect(registry.getByText('Discovered')).toBeInTheDocument()
    expect(registry.getByText(/27 May 2026/)).toBeInTheDocument()

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

  it('clears earlier refresh feedback after a successful configuration change', async () => {
    refreshExternalMcpServerDiscoveryMock.mockResolvedValueOnce({
      data: { server, status: 'failed', error_summary: 'Previous endpoint failed', tools: [] },
    })
    await renderServersTab('server_1')
    fireEvent.click(screen.getByRole('button', { name: 'Refresh GitHub' }))
    await waitFor(() => expect(screen.getByText('Previous endpoint failed')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: 'Edit GitHub' }))
    fireEvent.change(screen.getByLabelText('Server URL'), {
      target: { value: 'https://example.test/mcp' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }))
    await waitFor(() => expect(saveMcpServerMock).toHaveBeenCalled())
    await waitFor(() =>
      expect(screen.queryByText('Previous endpoint failed')).not.toBeInTheDocument(),
    )
    expect(screen.queryByText('Discovery failed')).not.toBeInTheDocument()
  })

  it('imports recommended servers through the server function', async () => {
    await renderServersTab()
    fireEvent.click(screen.getByRole('button', { name: 'Browse catalog' }))
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
    fireEvent.click(screen.getByRole('button', { name: 'Open GitHub' }))
    fireEvent.click(screen.getByRole('button', { name: 'Disable GitHub' }))

    await waitFor(() => {
      expect(disableExternalMcpServerMock).toHaveBeenCalledWith({
        data: { serverId: 'server_1' },
      })
    })
  })

  it('combines search with discovery filters without changing registry summary counts', async () => {
    const failed: McpServerView = {
      ...server,
      id: 'failed',
      display_name: 'Notion',
      server_key: 'notion',
      server_url: 'https://mcp.notion.com/mcp',
      description: 'Shared documents',
      last_discovery_status: 'failed',
      last_tool_count: 12,
    }
    const authRequired: McpServerView = {
      ...server,
      id: 'auth',
      display_name: 'Figma',
      server_key: 'figma',
      last_discovery_status: 'auth_required',
      last_tool_count: null,
    }
    const disabled: McpServerView = {
      ...failed,
      id: 'disabled',
      display_name: 'Legacy',
      status: 'disabled',
    }
    const unrun: McpServerView = {
      ...server,
      id: 'unrun',
      display_name: 'Exa',
      server_key: 'exa',
      last_discovery_status: null,
      last_discovery_at: null,
      last_tool_count: 0,
    }
    await renderServersTab(null, [server, failed, authRequired, disabled, unrun])

    expect(screen.getByRole('button', { name: '2 need attention' })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('radio', { name: 'Needs attention' }))
    expect(screen.getByText('Showing 2 of 5 servers')).toBeInTheDocument()
    const registry = within(screen.getByTestId('mcp-server-list'))
    expect(screen.getByTestId('mcp-server-list')).toHaveClass('min-w-0')
    expect(registry.getByTestId('mcp-server-table-scroll')).toHaveClass('overflow-x-auto')
    expect(registry.getByText('Discovery failed')).toBeInTheDocument()
    expect(registry.getByText('Authentication required')).toBeInTheDocument()
    expect(registry.getByText('12')).toBeInTheDocument()
    expect(registry.getByText('—')).toBeInTheDocument()
    expect(registry.queryByText('Legacy')).not.toBeInTheDocument()
    expect(registry.queryByText('Exa')).not.toBeInTheDocument()

    for (const search of [' NOTION ', 'mcp.notion.com', 'Shared documents']) {
      fireEvent.change(screen.getByRole('textbox', { name: 'Search servers' }), {
        target: { value: search },
      })
      expect(screen.getByText('Showing 1 of 5 servers')).toBeInTheDocument()
      expect(registry.getByText('Notion')).toBeInTheDocument()
    }
    expect(screen.getByRole('button', { name: '2 need attention' })).toBeInTheDocument()
    fireEvent.change(screen.getByRole('textbox', { name: 'Search servers' }), {
      target: { value: 'unknown' },
    })
    expect(screen.getByText('No matching servers')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Clear filters' }))
    expect(screen.getByText('Showing 5 of 5 servers')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('radio', { name: 'Disabled' }))
    expect(screen.getByText('Showing 1 of 5 servers')).toBeInTheDocument()
    expect(
      within(screen.getByTestId('mcp-server-list')).getByRole('button', { name: 'Refresh Legacy' }),
    ).toBeDisabled()
  })

  it('retains discovery, count, endpoint, and authentication details in mobile rows', async () => {
    await renderServersTab()
    const mobile = within(screen.getByTestId('mcp-server-list-mobile'))
    expect(mobile.getByText('api.githubcopilot.com')).toBeInTheDocument()
    expect(mobile.getByText('Gateway bearer token')).toBeInTheDocument()
    expect(mobile.getByText('Discovered')).toBeInTheDocument()
    expect(mobile.getByText(/27 May 2026/)).toBeInTheDocument()
    expect(mobile.getByText('1 tool')).toBeInTheDocument()
    fireEvent.click(mobile.getByRole('button', { name: 'Manage GitHub' }))
    expect(screen.getByRole('dialog', { name: 'Manage MCP server' })).toBeInTheDocument()
  })

  it('uses deterministic UTC discovery timestamps with unknown and missing date fallbacks', async () => {
    await renderServersTab(null, [
      { ...server, last_discovery_at: '2026-09-05T17:09:00+01:00' },
      { ...server, id: 'invalid', display_name: 'Invalid date', last_discovery_at: 'invalid' },
      { ...server, id: 'unrun', display_name: 'Not discovered', last_discovery_at: null },
    ])
    const registry = within(screen.getByTestId('mcp-server-list'))
    expect(registry.getByText('5 Sep 2026, 16:09 UTC')).toHaveAttribute(
      'datetime',
      '2026-09-05T17:09:00+01:00',
    )
    expect(registry.getByText('Unknown discovery time')).toBeInTheDocument()
    expect(registry.getByText('No discovery yet')).toBeInTheDocument()
  })

  it('preserves sorting when a search has no matching rows', async () => {
    const second = { ...server, id: 'second', display_name: 'Alpha', server_key: 'alpha' }
    await renderServersTab(null, [server, second])
    const registry = within(screen.getByTestId('mcp-server-list'))
    fireEvent.click(registry.getByRole('button', { name: 'Server' }))
    expect(registry.getAllByRole('button', { name: /^Open / })[0]).toHaveAccessibleName(
      'Open Alpha',
    )
    fireEvent.change(screen.getByRole('textbox', { name: 'Search servers' }), {
      target: { value: 'no match' },
    })
    expect(screen.getByText('No matching servers')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Clear filters' }))
    expect(
      within(screen.getByTestId('mcp-server-list')).getAllByRole('button', { name: /^Open / })[0],
    ).toHaveAccessibleName('Open Alpha')
  })

  it('keeps refresh pending on its server until both the request and invalidation finish', async () => {
    let finishRefresh!: (value: unknown) => void
    let finishInvalidation!: () => void
    refreshExternalMcpServerDiscoveryMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishRefresh = resolve
        }),
    )
    invalidateMock.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finishInvalidation = resolve
        }),
    )
    const second = { ...server, id: 'second', display_name: 'Notion', server_key: 'notion' }
    await renderServersTab(null, [server, second])
    const registry = within(screen.getByTestId('mcp-server-list'))
    fireEvent.click(registry.getByRole('button', { name: 'Refresh GitHub' }))
    expect(registry.getByRole('button', { name: 'Refresh GitHub' })).toBeDisabled()
    expect(registry.getByRole('status', { name: 'Refreshing GitHub' })).toBeInTheDocument()
    expect(registry.getByRole('button', { name: 'Refresh Notion' })).toBeEnabled()

    await act(async () => {
      finishRefresh({
        data: { server, status: 'success', error_summary: null, tools: [activeTool] },
      })
    })
    expect(registry.getByRole('button', { name: 'Refresh GitHub' })).toBeDisabled()
    await act(async () => {
      finishInvalidation()
    })
    expect(registry.getByRole('button', { name: 'Refresh GitHub' })).toBeEnabled()
  })

  it('does not replace another server tools or diagnostics when a row refresh finishes', async () => {
    let finishRefresh!: (value: unknown) => void
    refreshExternalMcpServerDiscoveryMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishRefresh = resolve
        }),
    )
    const second = { ...server, id: 'second', display_name: 'Notion', server_key: 'notion' }
    getMcpServerToolsMock.mockResolvedValue({
      data: { items: [{ ...activeTool, server_id: 'second' }] },
    })
    await renderServersTab(null, [server, second])
    const registry = within(screen.getByTestId('mcp-server-list'))
    fireEvent.click(registry.getByRole('button', { name: 'Refresh GitHub' }))
    fireEvent.click(registry.getByRole('button', { name: 'Manage Notion' }))
    fireEvent.click(screen.getAllByRole('button', { name: 'Tools' })[0])
    await waitFor(() => expect(screen.getByText('query_docs')).toBeInTheDocument())

    await act(async () => {
      finishRefresh({
        data: { server, status: 'failed', error_summary: 'GitHub failed', tools: [tool] },
      })
    })
    expect(screen.getByText('query_docs')).toBeInTheDocument()
    expect(screen.queryByText('Create issue')).not.toBeInTheDocument()
    fireEvent.click(screen.getAllByRole('button', { name: 'Overview' })[0])
    expect(screen.queryByText('GitHub failed')).not.toBeInTheDocument()
  })

  it('reloads inactive tools after discovery instead of replacing them with its active-only result', async () => {
    getMcpServerToolsMock.mockResolvedValue({ data: { items: [activeTool, tool] } })
    refreshExternalMcpServerDiscoveryMock.mockResolvedValueOnce({
      data: { server, status: 'success', error_summary: null, tools: [activeTool] },
    })
    await renderServersTab('server_1')
    await waitFor(() => expect(getMcpServerToolsMock).toHaveBeenCalledTimes(1))
    fireEvent.click(screen.getByRole('button', { name: 'Refresh GitHub' }))
    await waitFor(() => expect(getMcpServerToolsMock).toHaveBeenCalledTimes(2))
    expect(getMcpServerToolsMock).toHaveBeenLastCalledWith({
      data: { serverId: 'server_1', include_inactive: true },
    })
    fireEvent.click(screen.getAllByRole('button', { name: 'Tools' })[0])
    await waitFor(() => expect(screen.getByText('Create issue')).toBeInTheDocument())
    expect(screen.getByRole('checkbox', { name: 'Select create_issue' })).toBeDisabled()
  })

  it('opens catalog customization with prefilled values and prevents duplicate direct imports', async () => {
    await renderServersTab(null, [{ ...server, server_key: 'linear', status: 'disabled' }])
    expect(screen.queryByText('Recommended catalog')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Browse catalog' }))
    expect(screen.getByRole('button', { name: 'Registered' })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'Customize' }))
    expect(screen.queryByRole('dialog', { name: 'Browse MCP catalog' })).not.toBeInTheDocument()
    expect(screen.getByLabelText('Server key')).toHaveValue('linear')
    expect(screen.getByLabelText('Display name')).toHaveValue('Linear')
    expect(screen.getByLabelText('Server URL')).toHaveValue('https://mcp.linear.app/mcp')
  })
})
