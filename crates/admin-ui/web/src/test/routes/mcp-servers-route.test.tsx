import { useState, type MouseEvent } from 'react'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { toast, type Action } from 'sonner'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'

import { TooltipProvider } from '@/components/ui/tooltip'
import type { McpWorkspaceScope } from '@/routes/mcp/-scope'
import type { McpServerView, McpToolView, RecommendedMcpServerView } from '@/types/api'

const navigateMock = vi.fn()
const invalidateMock = vi.fn()
const getMcpServerToolsMock = vi.fn()
const getMcpCredentialBindingsMock = vi.fn()
const addMcpServerMock = vi.fn()
const saveMcpServerMock = vi.fn()
const disableExternalMcpServerMock = vi.fn()
const enableExternalMcpServerMock = vi.fn()
const refreshExternalMcpServerDiscoveryMock = vi.fn()
const discoverExternalMcpServerOauthMock = vi.fn()
const transferExternalMcpServerOwnershipMock = vi.fn()
const saveMcpToolTagsMock = vi.fn()
const saveMcpToolAnnotationOverridesMock = vi.fn()

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
  discoverExternalMcpServerOauth: (...args: unknown[]) =>
    discoverExternalMcpServerOauthMock(...args),
  enableExternalMcpServer: (...args: unknown[]) => enableExternalMcpServerMock(...args),
  getMcpCredentialBindings: (...args: unknown[]) => getMcpCredentialBindingsMock(...args),
  getMcpServerTools: (...args: unknown[]) => getMcpServerToolsMock(...args),
  refreshExternalMcpServerDiscovery: (...args: unknown[]) =>
    refreshExternalMcpServerDiscoveryMock(...args),
  removeMcpCredentialBinding: vi.fn(),
  saveMcpCredentialBinding: vi.fn(),
  saveMcpServer: (...args: unknown[]) => saveMcpServerMock(...args),
  transferExternalMcpServerOwnership: (...args: unknown[]) =>
    transferExternalMcpServerOwnershipMock(...args),
}))

vi.mock('@/server/mcp-policy-data.functions', () => ({
  saveMcpToolTags: (...args: unknown[]) => saveMcpToolTagsMock(...args),
  saveMcpToolAnnotationOverrides: (...args: unknown[]) =>
    saveMcpToolAnnotationOverridesMock(...args),
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
  owner_team_id: null,
  owner_team_name: null,
  owner_contact: 'platform@example.test',
  visibility: 'organisation',
  can_manage: true,
  approve_tool_changes: false,
}

const tool: McpToolView = {
  id: 'tool_1',
  server_id: 'server_1',
  upstream_name: 'create_issue',
  display_name: 'create_issue',
  title: null,
  description: 'Create issue',
  input_schema: {},
  output_schema: null,
  annotations: {
    read_only_hint: null,
    destructive_hint: null,
    idempotent_hint: null,
    open_world_hint: null,
  },
  annotation_overrides: {
    read_only_hint: null,
    destructive_hint: null,
    idempotent_hint: null,
    open_world_hint: null,
  },
  effective_annotations: {
    read_only_hint: null,
    destructive_hint: null,
    idempotent_hint: null,
    open_world_hint: null,
  },
  admin_tags: [],
  effective_tags: ['destructive', 'open_world'],
  approval_status: 'approved',
  approved_schema_version: 2,
  pending_schema_version: null,
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
  approved_schema_version: 3,
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

const teamServer: McpServerView = {
  ...server,
  id: 'server_team',
  server_key: 'docs',
  display_name: 'Docs',
  server_url: 'https://docs.example.test/mcp',
  owner_team_id: 'team_a',
  owner_team_name: 'Team A',
  owner_contact: '#team-a',
  visibility: 'team',
}

const teamAdminScope: McpWorkspaceScope = {
  platformAdmin: false,
  canPublish: false,
  teamId: 'team_a',
  teams: [],
  readOnly: false,
}

async function renderServersTab(
  initialSelectedServerId: string | null = null,
  servers: McpServerView[] = [server],
  scope?: McpWorkspaceScope,
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
        scope={scope}
        onSelectServer={setSelectedServerId}
        onAddToToolset={onAddToToolset}
      />
    )
  }

  render(<ServersTabHarness />, { wrapper: TooltipProvider })

  return { onAddToToolset }
}

describe('ServersTab', () => {
  // The first import compiles the tab and every MCP module it uses, which can
  // take longer than one test's timeout on a busy runner. Load it once here so
  // no test pays for it.
  beforeAll(async () => {
    await import('@/routes/mcp/-servers-tab')
  }, 30_000)

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
    enableExternalMcpServerMock.mockReset()
    refreshExternalMcpServerDiscoveryMock.mockReset()
    navigateMock.mockReset()
    invalidateMock.mockReset()
    vi.mocked(toast.error).mockClear()
    vi.mocked(toast.success).mockClear()

    getMcpServerToolsMock.mockResolvedValue({ data: { items: [tool] } })
    getMcpCredentialBindingsMock.mockResolvedValue({ data: { items: [] } })
    addMcpServerMock.mockResolvedValue({ data: { server } })
    saveMcpServerMock.mockResolvedValue({ data: { server } })
    disableExternalMcpServerMock.mockResolvedValue({
      data: { server: { ...server, status: 'disabled' } },
    })
    enableExternalMcpServerMock.mockResolvedValue({
      data: { server: { ...server, last_discovery_status: null } },
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

  it('shows declared annotation hints, title, and output schema', async () => {
    const annotatedTools: McpToolView[] = [
      {
        ...activeTool,
        id: 'tool_read',
        title: 'Query documentation',
        output_schema: { type: 'object', properties: { answer: { type: 'string' } } },
        annotations: {
          read_only_hint: true,
          destructive_hint: true,
          idempotent_hint: true,
          open_world_hint: false,
        },
      },
      {
        ...activeTool,
        id: 'tool_delete',
        upstream_name: 'delete_page',
        display_name: 'delete_page',
        annotations: {
          read_only_hint: false,
          destructive_hint: true,
          idempotent_hint: null,
          open_world_hint: true,
        },
      },
      {
        ...activeTool,
        id: 'tool_append',
        upstream_name: 'append_note',
        display_name: 'append_note',
        annotations: {
          read_only_hint: null,
          destructive_hint: false,
          idempotent_hint: null,
          open_world_hint: null,
        },
      },
      { ...tool, id: 'tool_plain', upstream_name: 'plain', display_name: 'plain' },
    ]
    getMcpServerToolsMock.mockResolvedValueOnce({ data: { items: annotatedTools } })
    await renderServersTab()

    fireEvent.click(screen.getByRole('button', { name: 'Open GitHub' }))
    fireEvent.click(screen.getAllByRole('button', { name: 'Tools' })[0])
    await waitFor(() => expect(screen.getByText('query_docs')).toBeInTheDocument())
    // Collapsed rows keep to the name, description and status; hints show when expanded.
    expect(screen.queryByTestId('mcp-tool-annotations')).not.toBeInTheDocument()

    for (const name of ['query_docs', 'delete_page', 'append_note', 'plain']) {
      fireEvent.click(screen.getByRole('button', { name: `Show ${name} schema` }))
    }
    const [readBadges, deleteBadges, appendBadges] = screen.getAllByTestId('mcp-tool-annotations')
    expect(within(readBadges).getByText('Read-only')).toBeInTheDocument()
    expect(within(readBadges).getByText('Idempotent')).toBeInTheDocument()
    expect(within(readBadges).queryByText('Destructive')).not.toBeInTheDocument()
    expect(within(readBadges).queryByText('Open world')).not.toBeInTheDocument()
    expect(within(readBadges).getByText('Closed world')).toBeInTheDocument()
    expect(within(deleteBadges).getByText('Destructive')).toBeInTheDocument()
    expect(within(deleteBadges).getByText('Open world')).toBeInTheDocument()
    expect(within(deleteBadges).queryByText('Closed world')).not.toBeInTheDocument()
    expect(within(deleteBadges).queryByText('Non-destructive')).not.toBeInTheDocument()
    expect(within(appendBadges).getByText('Non-destructive')).toBeInTheDocument()
    expect(within(appendBadges).queryByText('Destructive')).not.toBeInTheDocument()
    expect(screen.getAllByTestId('mcp-tool-annotations')).toHaveLength(3)

    expect(screen.getByText('Query documentation')).toBeInTheDocument()
    expect(screen.getByText('Output schema')).toBeInTheDocument()
    expect(screen.getByTestId('mcp-tool-output-schema-code')).toHaveTextContent('answer')
  })

  it('shows upstream hints next to overrides and saves admin tags', async () => {
    const readTool: McpToolView = {
      ...activeTool,
      annotations: { ...activeTool.annotations, read_only_hint: true },
      effective_tags: ['open_world', 'read_only'],
    }
    getMcpServerToolsMock.mockResolvedValueOnce({ data: { items: [readTool] } })
    saveMcpToolTagsMock.mockResolvedValueOnce({
      data: {
        tool: {
          ...readTool,
          admin_tags: ['docs', 'finance'],
          effective_tags: ['docs', 'finance', 'open_world', 'read_only'],
        },
      },
    })
    saveMcpToolAnnotationOverridesMock.mockResolvedValueOnce({ data: { tool: readTool } })
    await renderServersTab()

    fireEvent.click(screen.getByRole('button', { name: 'Open GitHub' }))
    fireEvent.click(screen.getAllByRole('button', { name: 'Tools' })[0])
    await waitFor(() => expect(screen.getByText('query_docs')).toBeInTheDocument())
    expect(screen.queryByTestId('mcp-tool-policy')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Show query_docs schema' }))

    const overrides = screen.getByTestId('mcp-tool-annotation-overrides')
    expect(within(overrides).getByText('Upstream: Yes')).toBeInTheDocument()
    expect(
      within(screen.getByTestId('mcp-tool-effective-tags')).getByText('read_only'),
    ).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Save overrides' }))
    await waitFor(() =>
      expect(saveMcpToolAnnotationOverridesMock).toHaveBeenCalledWith({
        data: {
          serverId: 'server_1',
          toolId: 'tool_2',
          annotationOverrides: {
            read_only_hint: null,
            destructive_hint: null,
            idempotent_hint: null,
            open_world_hint: null,
          },
        },
      }),
    )

    fireEvent.change(screen.getByLabelText('Admin tags'), {
      target: { value: 'finance, docs' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Save tags' }))
    await waitFor(() =>
      expect(saveMcpToolTagsMock).toHaveBeenCalledWith({
        data: { serverId: 'server_1', toolId: 'tool_2', tags: ['finance', 'docs'] },
      }),
    )
    await waitFor(() =>
      expect(
        within(screen.getByTestId('mcp-tool-effective-tags')).getByText('finance'),
      ).toBeInTheDocument(),
    )
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

  it('refreshes discovery and reports the result in the overview without a toast', async () => {
    await renderServersTab('server_1')
    fireEvent.click(screen.getByRole('button', { name: 'Refresh GitHub' }))

    await waitFor(() => {
      expect(refreshExternalMcpServerDiscoveryMock).toHaveBeenCalledWith({
        data: { serverId: 'server_1' },
      })
    })
    const alert = await screen.findByRole('alert')
    expect(within(alert).getByText('Discovery refreshed')).toBeInTheDocument()
    expect(within(alert).getByText('Found 1 tool.')).toBeInTheDocument()
    expect(within(alert).queryByRole('button', { name: 'Retry' })).not.toBeInTheDocument()
    expect(toast.success).not.toHaveBeenCalled()
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
    expect(
      screen.getByText(
        "Discovery didn't complete for api.githubcopilot.com. Check the endpoint and auth config, then Retry.",
      ),
    ).toBeInTheDocument()
    expect(screen.getByText('new upstream failure')).toBeInTheDocument()
    expect(screen.queryByText('old upstream failure')).not.toBeInTheDocument()
  })

  it('explains an unreachable endpoint once, in the overview, and retries from there', async () => {
    const unreachable = { ...server, server_url: 'https://unreachable.invalid/mcp' }
    const failure = {
      data: {
        server: unreachable,
        status: 'failed',
        error_kind: 'transport',
        error_summary: 'MCP transport failure: error sending request',
        tools: [],
      },
    }
    const explanation = "Couldn't reach unreachable.invalid. Check the endpoint, then Retry."
    refreshExternalMcpServerDiscoveryMock.mockResolvedValueOnce(failure)
    await renderServersTab('server_1', [unreachable])
    fireEvent.click(screen.getByRole('button', { name: 'Refresh GitHub' }))

    const alert = await screen.findByRole('alert')
    expect(within(alert).getByText('Discovery failed')).toBeInTheDocument()
    expect(within(alert).getByText(explanation)).toBeInTheDocument()
    expect(
      within(alert).getByText('MCP transport failure: error sending request'),
    ).toBeInTheDocument()
    expect(toast.error).not.toHaveBeenCalled()

    let finishRetry!: (value: unknown) => void
    refreshExternalMcpServerDiscoveryMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishRetry = resolve
        }),
    )
    fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }))
    expect(refreshExternalMcpServerDiscoveryMock).toHaveBeenCalledTimes(2)
    // The explanation stays up while the retry runs.
    expect(within(alert).getByRole('button', { name: 'Retry' })).toBeDisabled()
    expect(within(alert).getByText(explanation)).toBeInTheDocument()

    await act(async () => {
      finishRetry(failure)
    })
    expect(within(screen.getByRole('alert')).getByRole('button', { name: 'Retry' })).toBeEnabled()
    expect(screen.getAllByText(explanation)).toHaveLength(1)
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('toasts a refresh result only while its overview is hidden, linking back to it', async () => {
    refreshExternalMcpServerDiscoveryMock.mockResolvedValue({
      data: {
        server,
        status: 'failed',
        error_kind: 'transport',
        error_summary: 'MCP transport failure: error sending request',
        tools: [],
      },
    })
    await renderServersTab()
    const registry = within(screen.getByTestId('mcp-server-list'))
    fireEvent.click(registry.getByRole('button', { name: 'Refresh GitHub' }))

    await waitFor(() => expect(toast.error).toHaveBeenCalledTimes(1))
    expect(toast.error).toHaveBeenCalledWith('Discovery failed for GitHub', {
      action: expect.objectContaining({ label: 'View' }),
    })
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()

    const [, options] = vi.mocked(toast.error).mock.calls[0]
    act(() => (options!.action as Action).onClick({} as MouseEvent<HTMLButtonElement>))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      "Couldn't reach api.githubcopilot.com. Check the endpoint, then Retry.",
    )

    // The header refresh still runs from other sections, where the overview is out of sight.
    fireEvent.click(screen.getAllByRole('button', { name: 'Tools' })[0])
    fireEvent.click(screen.getByRole('button', { name: 'Refresh GitHub' }))
    await waitFor(() => expect(toast.error).toHaveBeenCalledTimes(2))
  })

  it('gives every header icon action a tooltip', async () => {
    await renderServersTab('server_1')

    for (const [name, label] of [
      ['Refresh GitHub', 'Refresh tools'],
      ['Edit GitHub', 'Edit configuration'],
      ['More actions for GitHub', 'More actions'],
      ['Close', 'Close'],
    ]) {
      const button = screen.getByRole('button', { name })
      fireEvent.focus(button)
      expect(await screen.findByRole('tooltip')).toHaveTextContent(label)
      fireEvent.blur(button)
      await waitFor(() => expect(screen.queryByRole('tooltip')).not.toBeInTheDocument())
    }
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

    // The add dialog stays open, hiding the list, until its transition finishes.
    fireEvent.click(await screen.findByRole('button', { name: 'Edit GitHub' }))
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

  it('fills OAuth settings from discovery and lets the admin edit them before saving', async () => {
    discoverExternalMcpServerOauthMock.mockResolvedValue({
      data: {
        resource: 'https://mcp.linear.example/mcp',
        resource_metadata_url:
          'https://mcp.linear.example/.well-known/oauth-protected-resource/mcp',
        authorization_servers: ['https://auth.linear.example'],
        issuer: 'https://auth.linear.example',
        authorization_server_metadata_url:
          'https://auth.linear.example/.well-known/oauth-authorization-server',
        authorization_endpoint: 'https://auth.linear.example/authorize',
        token_endpoint: 'https://auth.linear.example/token',
        registration_endpoint: 'https://auth.linear.example/register',
        scopes_supported: ['read', 'write'],
        default_scopes: ['read'],
        token_endpoint_auth_methods_supported: ['client_secret_basic'],
        code_challenge_methods_supported: ['S256'],
        client_id_metadata_document_supported: false,
        recommended_client_registration: 'dynamic',
        resource_indicator: true,
        callback_url: 'https://gateway.example/api/v1/mcp/oauth/callback',
        client_metadata_document_url: null,
        warnings: ['The server lists 2 authorization servers; Oceans uses the first.'],
        suggested_auth_config: {
          resource: 'https://mcp.linear.example/mcp',
          issuer: 'https://auth.linear.example',
          client_registration: 'dynamic',
          scopes: ['read'],
        },
      },
    })
    await renderServersTab()
    fireEvent.click(screen.getByRole('button', { name: 'Add server' }))
    fireEvent.change(screen.getByLabelText('Server key'), { target: { value: 'linear' } })
    fireEvent.change(screen.getByLabelText('Display name'), { target: { value: 'Linear' } })
    fireEvent.change(screen.getByLabelText('Server URL'), {
      target: { value: 'https://mcp.linear.example/mcp' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Discover OAuth' }))

    await waitFor(() =>
      expect(discoverExternalMcpServerOauthMock).toHaveBeenCalledWith({
        data: { serverUrl: 'https://mcp.linear.example/mcp' },
      }),
    )
    // The panel holds the Discover OAuth button, so it renders before discovery resolves.
    const panel = screen.getByTestId('mcp-oauth-discovery')
    expect(await within(panel).findByText('Dynamic client registration')).toBeTruthy()
    expect(within(panel).getByText('https://auth.linear.example/token')).toBeTruthy()
    expect(within(panel).getByText(/lists 2 authorization servers/)).toBeTruthy()

    fireEvent.click(within(panel).getByRole('button', { name: 'Use discovered settings' }))
    const authConfig = screen.getByLabelText('Auth config JSON') as HTMLTextAreaElement
    const applied = JSON.parse(authConfig.value) as Record<string, unknown>
    expect(applied.client_registration).toBe('dynamic')
    // The admin overrides the discovered scopes before saving.
    fireEvent.change(authConfig, {
      target: { value: JSON.stringify({ ...applied, scopes: ['read', 'write'] }) },
    })
    const addButtons = screen.getAllByRole('button', { name: 'Add server' })
    fireEvent.click(addButtons[addButtons.length - 1])

    await waitFor(() => {
      expect(addMcpServerMock).toHaveBeenCalledWith({
        data: expect.objectContaining({
          server_key: 'linear',
          auth_mode: 'oauth_obo',
          auth_config: expect.objectContaining({
            issuer: 'https://auth.linear.example',
            scopes: ['read', 'write'],
          }),
        }),
      })
    })
  })

  it('disables active servers from the more actions menu', async () => {
    await renderServersTab()
    fireEvent.click(screen.getByRole('button', { name: 'Open GitHub' }))
    expect(screen.queryByRole('button', { name: 'Disable GitHub' })).not.toBeInTheDocument()

    fireEvent.pointerDown(screen.getByRole('button', { name: 'More actions for GitHub' }))
    expect(screen.queryByRole('menuitem', { name: 'Enable server' })).not.toBeInTheDocument()
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Disable server' }))

    await waitFor(() => {
      expect(disableExternalMcpServerMock).toHaveBeenCalledWith({
        data: { serverId: 'server_1' },
      })
    })
  })

  it('does not reopen the more actions tooltip when the menu hands focus back', async () => {
    await renderServersTab('server_1')
    const trigger = screen.getByRole('button', { name: 'More actions for GitHub' })
    fireEvent.pointerDown(trigger)
    fireEvent.pointerUp(trigger)
    const menu = await screen.findByRole('menu')
    fireEvent.keyDown(menu, { key: 'Escape' })

    await waitFor(() => expect(screen.queryByRole('menu')).not.toBeInTheDocument())
    expect(trigger).toHaveFocus()
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument()
  })

  it('enables a disabled server from the more actions menu', async () => {
    await renderServersTab('server_1', [{ ...server, status: 'disabled' }])
    expect(screen.getByRole('button', { name: 'Refresh GitHub' })).toBeDisabled()

    fireEvent.pointerDown(screen.getByRole('button', { name: 'More actions for GitHub' }))
    expect(await screen.findByRole('menuitem', { name: 'Enable server' })).toBeInTheDocument()
    expect(screen.queryByRole('menuitem', { name: 'Disable server' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('menuitem', { name: 'Enable server' }))

    await waitFor(() => {
      expect(enableExternalMcpServerMock).toHaveBeenCalledWith({ data: { serverId: 'server_1' } })
    })
    expect(disableExternalMcpServerMock).not.toHaveBeenCalled()
    await waitFor(() => expect(toast.success).toHaveBeenCalledWith('MCP server enabled'))
    expect(invalidateMock).toHaveBeenCalled()
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

  it('labels discovery in the server details and marks failures while the server is active', async () => {
    const failed: McpServerView = {
      ...server,
      id: 'failed',
      display_name: 'Notion',
      server_key: 'notion',
      last_discovery_status: 'failed',
    }
    const authRequired: McpServerView = {
      ...server,
      id: 'auth',
      display_name: 'Figma',
      server_key: 'figma',
      last_discovery_status: 'auth_required',
    }
    const disabled: McpServerView = {
      ...failed,
      id: 'disabled',
      display_name: 'Legacy',
      server_key: 'legacy',
      status: 'disabled',
    }
    const unrun: McpServerView = {
      ...server,
      id: 'unrun',
      display_name: 'Exa',
      server_key: 'exa',
      last_discovery_status: null,
      last_discovery_at: null,
    }
    const servers = [server, failed, authRequired, disabled, unrun]
    const headerBadges = (name: string) => {
      const header = screen.getByRole('heading', { level: 2, name }).parentElement as HTMLElement
      return [...header.querySelectorAll('[data-slot="badge"]')].map((badge) => [
        badge.textContent,
        badge.getAttribute('data-variant'),
      ])
    }

    for (const [selected, name, badges] of [
      [
        'failed',
        'Notion',
        [
          ['Active', 'success'],
          ['Discovery failed', 'destructive'],
        ],
      ],
      [
        'auth',
        'Figma',
        [
          ['Active', 'success'],
          ['Authentication required', 'destructive'],
        ],
      ],
      [
        'disabled',
        'Legacy',
        [
          ['Disabled', 'secondary'],
          ['Discovery failed', 'secondary'],
        ],
      ],
      [
        'unrun',
        'Exa',
        [
          ['Active', 'success'],
          ['Not discovered', 'outline'],
        ],
      ],
      // A successful discovery adds nothing beside the registration badge.
      ['server_1', 'GitHub', [['Active', 'success']]],
    ] as const) {
      await renderServersTab(selected, servers)
      expect(headerBadges(name)).toEqual(badges)
      cleanup()
    }
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
    expect(toast.error).toHaveBeenCalledWith('Discovery failed for GitHub', expect.anything())
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

  it('shows each server owner and visibility, and filters by owner', async () => {
    await renderServersTab(null, [server, teamServer])

    const registry = within(screen.getByTestId('mcp-server-list'))
    expect(registry.getByText('Platform admins')).toBeInTheDocument()
    expect(registry.getByText('Team A')).toHaveAttribute('title', '#team-a')
    expect(registry.getByText('Organisation')).toBeInTheDocument()
    expect(registry.getByText('Team only')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('combobox', { name: 'Filter by owner' }))
    fireEvent.click(await screen.findByRole('option', { name: 'Team A' }))

    await waitFor(() => expect(registry.queryByText('GitHub')).not.toBeInTheDocument())
    expect(registry.getByText('Docs')).toBeInTheDocument()
    expect(screen.getByText('Showing 1 of 2 servers')).toBeInTheDocument()
  })

  it('keeps servers a team admin does not manage read-only', async () => {
    const published = { ...server, can_manage: false }
    await renderServersTab(null, [published, teamServer], teamAdminScope)

    const registry = within(screen.getByTestId('mcp-server-list'))
    expect(registry.getByRole('button', { name: 'View GitHub' })).toBeInTheDocument()
    expect(registry.getByRole('button', { name: 'Manage Docs' })).toBeInTheDocument()
    expect(registry.getByRole('button', { name: 'Refresh GitHub' })).toBeDisabled()

    fireEvent.click(screen.getByRole('button', { name: 'Open GitHub' }))
    expect(
      await screen.findByText(/This server is managed by Platform admins\./),
    ).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Edit GitHub' })).not.toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: 'More actions for GitHub' }),
    ).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Close' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Configuration' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Credentials' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Transfer ownership' })).not.toBeInTheDocument()
    expect(getMcpCredentialBindingsMock).not.toHaveBeenCalled()
  })

  it('lets a team admin re-enable a server their team owns', async () => {
    await renderServersTab(
      'server_team',
      [{ ...teamServer, status: 'disabled' }, server],
      teamAdminScope,
    )

    fireEvent.pointerDown(screen.getByRole('button', { name: 'More actions for Docs' }))
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Enable server' }))

    await waitFor(() => {
      expect(enableExternalMcpServerMock).toHaveBeenCalledWith({
        data: { serverId: 'server_team' },
      })
    })
  })

  it('registers a team admin server as team only', async () => {
    await renderServersTab(null, [teamServer], teamAdminScope)
    fireEvent.click(screen.getByRole('button', { name: 'Add server' }))
    expect(
      screen.getByText('Only a platform admin can publish this server to the organisation.'),
    ).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('Server key'), { target: { value: 'wiki' } })
    fireEvent.change(screen.getByLabelText('Display name'), { target: { value: 'Wiki' } })
    fireEvent.change(screen.getByLabelText('Server URL'), {
      target: { value: 'https://wiki.example.test/mcp' },
    })
    fireEvent.change(screen.getByLabelText('Owner contact'), { target: { value: '#wiki' } })
    const addButtons = screen.getAllByRole('button', { name: 'Add server' })
    fireEvent.click(addButtons[addButtons.length - 1])

    await waitFor(() => {
      expect(addMcpServerMock).toHaveBeenCalledWith({
        data: expect.objectContaining({
          server_key: 'wiki',
          visibility: 'team',
          owner_contact: '#wiki',
        }),
      })
    })
  })

  it('lets a platform admin move a server to a team', async () => {
    transferExternalMcpServerOwnershipMock.mockResolvedValue({
      data: { server: { ...server, owner_team_id: 'team_a', owner_team_name: 'Team A' } },
    })
    await renderServersTab('server_1', [server], {
      platformAdmin: true,
      canPublish: true,
      teamId: null,
      teams: [{ id: 'team_a', name: 'Team A' }],
      readOnly: false,
    })

    fireEvent.click(await screen.findByRole('combobox', { name: 'Owning team' }))
    fireEvent.click(await screen.findByRole('option', { name: 'Team A' }))
    fireEvent.click(screen.getByRole('button', { name: 'Transfer ownership' }))

    await waitFor(() => {
      expect(transferExternalMcpServerOwnershipMock).toHaveBeenCalledWith({
        data: { serverId: 'server_1', input: { owner_team_id: 'team_a' } },
      })
    })
  })

  it('lets a platform admin register a server for a team', async () => {
    await renderServersTab(null, [server], {
      platformAdmin: true,
      canPublish: true,
      teamId: null,
      teams: [{ id: 'team_a', name: 'Team A' }],
      readOnly: false,
    })
    fireEvent.click(screen.getByRole('button', { name: 'Add server' }))
    fireEvent.click(screen.getByRole('combobox', { name: 'Owning team' }))
    fireEvent.click(await screen.findByRole('option', { name: 'Team A' }))
    expect(screen.getByRole('combobox', { name: 'Visibility' })).toHaveTextContent('Team only')
    fireEvent.change(screen.getByLabelText('Server key'), { target: { value: 'docs' } })
    fireEvent.change(screen.getByLabelText('Display name'), { target: { value: 'Docs' } })
    fireEvent.change(screen.getByLabelText('Server URL'), {
      target: { value: 'https://docs.example.test/mcp' },
    })
    const addButtons = screen.getAllByRole('button', { name: 'Add server' })
    fireEvent.click(addButtons[addButtons.length - 1])

    await waitFor(() => {
      expect(addMcpServerMock).toHaveBeenCalledWith({
        data: expect.objectContaining({ owner_team_id: 'team_a', visibility: 'team' }),
      })
    })
  })

  it('publishes a server again when a platform admin switches the owner back to the platform', async () => {
    await renderServersTab(null, [server], {
      platformAdmin: true,
      canPublish: true,
      teamId: null,
      teams: [{ id: 'team_a', name: 'Team A' }],
      readOnly: false,
    })
    fireEvent.click(screen.getByRole('button', { name: 'Add server' }))
    fireEvent.click(screen.getByRole('combobox', { name: 'Owning team' }))
    fireEvent.click(await screen.findByRole('option', { name: 'Team A' }))
    expect(screen.getByRole('combobox', { name: 'Visibility' })).toHaveTextContent('Team only')
    fireEvent.click(screen.getByRole('combobox', { name: 'Owning team' }))
    fireEvent.click(await screen.findByRole('option', { name: 'Platform admins' }))
    expect(screen.getByRole('combobox', { name: 'Visibility' })).toHaveTextContent('Organisation')
    fireEvent.change(screen.getByLabelText('Server key'), { target: { value: 'docs' } })
    fireEvent.change(screen.getByLabelText('Display name'), { target: { value: 'Docs' } })
    fireEvent.change(screen.getByLabelText('Server URL'), {
      target: { value: 'https://docs.example.test/mcp' },
    })
    const addButtons = screen.getAllByRole('button', { name: 'Add server' })
    fireEvent.click(addButtons[addButtons.length - 1])

    await waitFor(() => {
      expect(addMcpServerMock).toHaveBeenCalledWith({
        data: expect.objectContaining({ owner_team_id: null, visibility: 'organisation' }),
      })
    })
  })

  it('shows members the servers they can connect to without admin controls', async () => {
    await renderServersTab(null, [{ ...server, can_manage: false }], {
      ...teamAdminScope,
      readOnly: true,
    })

    expect(screen.getByText('The MCP servers your access lets you connect to.')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Add server' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Browse catalog' })).not.toBeInTheDocument()
    const registry = within(screen.getByTestId('mcp-server-list'))
    expect(registry.getByRole('button', { name: 'View GitHub' })).toBeInTheDocument()
  })

  it('offers no tool set action on a team-only server', async () => {
    getMcpServerToolsMock.mockResolvedValueOnce({ data: { items: [activeTool] } })
    await renderServersTab(null, [teamServer])
    fireEvent.click(
      within(screen.getByTestId('mcp-server-list')).getByRole('button', { name: 'Open Docs' }),
    )
    fireEvent.click(screen.getAllByRole('button', { name: 'Tools' })[0])

    await waitFor(() => expect(screen.getByText('query_docs')).toBeInTheDocument())
    expect(screen.queryByRole('checkbox', { name: 'Select query_docs' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Add to toolset' })).not.toBeInTheDocument()
  })

  it('offers no tool selection without a tool set action', async () => {
    await renderServersTab(null, [{ ...server, can_manage: false }], {
      ...teamAdminScope,
      readOnly: true,
    })
    fireEvent.click(
      within(screen.getByTestId('mcp-server-list')).getByRole('button', { name: 'Open GitHub' }),
    )
    fireEvent.click(screen.getAllByRole('button', { name: 'Tools' })[0])

    await waitFor(() => expect(screen.getByText('Create issue')).toBeInTheDocument())
    expect(screen.getByText('The tools this server offers.')).toBeInTheDocument()
    expect(screen.queryByRole('checkbox', { name: 'Select create_issue' })).not.toBeInTheDocument()
  })
})
