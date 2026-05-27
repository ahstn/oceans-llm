import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { McpServerView, McpToolView, RecommendedMcpServerView } from '@/types/api'

const navigateMock = vi.fn()
const invalidateMock = vi.fn()
const getMcpServerToolsMock = vi.fn()
const addMcpServerMock = vi.fn()
const saveMcpServerMock = vi.fn()
const disableExternalMcpServerMock = vi.fn()
const refreshExternalMcpServerDiscoveryMock = vi.fn()

const routeMock = {
  useLoaderData: vi.fn(),
  useSearch: vi.fn(),
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
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
  getMcpServers: vi.fn(),
  getMcpServerTools: (...args: unknown[]) => getMcpServerToolsMock(...args),
  getRecommendedMcpServers: vi.fn(),
  refreshExternalMcpServerDiscovery: (...args: unknown[]) =>
    refreshExternalMcpServerDiscoveryMock(...args),
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

describe('McpServersPage', () => {
  afterEach(() => {
    cleanup()
  })

  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
    routeMock.useSearch.mockReset()
    getMcpServerToolsMock.mockReset()
    addMcpServerMock.mockReset()
    saveMcpServerMock.mockReset()
    disableExternalMcpServerMock.mockReset()
    refreshExternalMcpServerDiscoveryMock.mockReset()
    navigateMock.mockReset()
    invalidateMock.mockReset()
    routeMock.useSearch.mockReturnValue({})
    routeMock.useLoaderData.mockReturnValue({
      servers: [server],
      recommended: [recommended],
    })
    getMcpServerToolsMock.mockResolvedValue({ data: { items: [tool] } })
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
    const { McpServersPage } = await import('@/routes/mcp/servers')

    render(<McpServersPage />)

    expect(screen.getByText('MCP Servers')).toBeInTheDocument()
    expect(screen.getByTestId('mcp-server-list')).toBeInTheDocument()
    expect(screen.getByText('/mcp/github')).toBeInTheDocument()
    await waitFor(() => expect(screen.getByText('sha256:abc123')).toBeInTheDocument())
    expect(screen.getByText('Inactive')).toBeInTheDocument()
    expect(screen.getByText('2')).toBeInTheDocument()
  })

  it('refreshes discovery and renders refresh feedback', async () => {
    const { McpServersPage } = await import('@/routes/mcp/servers')

    render(<McpServersPage />)
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }))

    await waitFor(() => {
      expect(refreshExternalMcpServerDiscoveryMock).toHaveBeenCalledWith({
        data: { serverId: 'server_1' },
      })
    })
    await waitFor(() => expect(screen.getByText('Discovery success')).toBeInTheDocument())
  })

  it('imports recommended servers through the server function', async () => {
    const { McpServersPage } = await import('@/routes/mcp/servers')

    render(<McpServersPage />)
    fireEvent.click(screen.getByRole('button', { name: 'Import' }))

    await waitFor(() => {
      expect(addMcpServerMock).toHaveBeenCalledWith({
        data: { recommended_catalog_key: 'linear' },
      })
    })
  })

  it('submits custom add and edit flows', async () => {
    const { McpServersPage } = await import('@/routes/mcp/servers')

    render(<McpServersPage />)
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

    fireEvent.click(screen.getByRole('button', { name: 'Edit' }))
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
    const { McpServersPage } = await import('@/routes/mcp/servers')

    render(<McpServersPage />)
    fireEvent.click(screen.getByRole('button', { name: 'Disable' }))

    await waitFor(() => {
      expect(disableExternalMcpServerMock).toHaveBeenCalledWith({
        data: { serverId: 'server_1' },
      })
    })
  })
})
