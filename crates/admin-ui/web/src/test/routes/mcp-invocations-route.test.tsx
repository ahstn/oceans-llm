import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { McpInvocationView } from '@/types/api'
import { McpInvocationsPage } from '@/routes/observability/mcp-invocations'

const getObservabilityMcpInvocationDetailMock = vi.fn()
const navigateMock = vi.fn()

// Hoisted with the mocks: the route module reads it as it loads.
const routeMock = vi.hoisted(() => ({
  useLoaderData: vi.fn(),
  useSearch: vi.fn(),
}))

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => ({
    navigate: navigateMock,
  }),
}))

vi.mock('@/server/admin-data.functions', () => ({
  getMcpInvocations: vi.fn(),
  getMcpServers: vi.fn().mockResolvedValue({
    data: { items: [{ server_key: 'github', display_name: 'GitHub' }] },
  }),
  getObservabilityMcpInvocationDetail: (...args: unknown[]) =>
    getObservabilityMcpInvocationDetailMock(...args),
}))

const item: McpInvocationView = {
  mcp_tool_invocation_id: 'mcpinv_1',
  request_id: 'req_1',
  api_key_id: 'api_key_1',
  user_id: null,
  team_id: 'team_1',
  owner_kind: 'team',
  server_id: 'server_1',
  server_display_key: 'github',
  server_display_name: 'GitHub',
  tool_id: 'tool_1',
  tool_display_key: 'create_issue',
  tool_display_name: 'Create issue',
  mcp_method: 'tools/call',
  status: 'success',
  policy_result: 'allowed',
  latency_ms: 120,
  error_code: null,
  has_payload: true,
  arguments_payload_redacted: true,
  arguments_payload_truncated: false,
  result_payload_redacted: true,
  result_payload_truncated: true,
  metadata: {},
  occurred_at: '2026-03-10T11:33:00Z',
}

describe('McpInvocationsPage', () => {
  afterEach(() => {
    cleanup()
  })

  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
    routeMock.useSearch.mockReset()
    getObservabilityMcpInvocationDetailMock.mockReset()
    navigateMock.mockReset()
    routeMock.useSearch.mockReturnValue({})
  })

  it('renders the filter toolbar and invocation records', () => {
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [item], page: 1, page_size: 25, total: 1 },
    })

    render(<McpInvocationsPage />)

    expect(screen.getByRole('heading', { level: 1, name: 'MCP invocations' })).toBeInTheDocument()
    const toolbar = within(screen.getByTestId('mcp-invocation-toolbar'))
    expect(toolbar.getByRole('button', { name: /^Filters/ })).toBeInTheDocument()
    expect(toolbar.getByRole('status')).toHaveTextContent('Showing 1 of 1')
    expect(screen.queryByRole('button', { name: /Apply filters/i })).not.toBeInTheDocument()

    const table = within(screen.getByTestId('mcp-invocations-table'))
    expect(table.getByText('10 Mar 2026, 11:33:00 UTC')).toBeInTheDocument()
    expect(table.getByText('GitHub')).toBeInTheDocument()
    expect(table.getByText('create_issue')).toBeInTheDocument()
    expect(table.getByText('Allowed')).toBeInTheDocument()
    expect(table.getByText('120 ms')).toBeInTheDocument()
    expect(table.getByText('result truncated')).toBeInTheDocument()
    expect(table.getByText('Team')).toBeInTheDocument()
    expect(table.getByText('team_1')).toBeInTheDocument()
  })

  it('shows the ID that matches the owner kind', () => {
    const userOwned = { ...item, owner_kind: 'user', user_id: 'user_1', team_id: 'team_1' }
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [userOwned], page: 1, page_size: 25, total: 1 },
    })

    render(<McpInvocationsPage />)

    const table = within(screen.getByTestId('mcp-invocations-table'))
    expect(table.getByText('User')).toBeInTheDocument()
    expect(table.getByText('user_1')).toBeInTheDocument()
    expect(table.queryByText('team_1')).not.toBeInTheDocument()
    expect(screen.queryByText(/n\/a/)).not.toBeInTheDocument()
  })

  it('truncates long server keys and keeps the full key in a tooltip', () => {
    const serverKey = 'verify-20261127t191841z-design2-d88e1-context7'
    routeMock.useLoaderData.mockReturnValue({
      data: {
        items: [{ ...item, server_display_key: serverKey }],
        page: 1,
        page_size: 25,
        total: 1,
      },
    })

    render(<McpInvocationsPage />)

    const key = within(screen.getByTestId('mcp-invocations-table')).getByTitle(serverKey)
    expect(key).toHaveClass('truncate')
    expect(key.parentElement).toHaveClass('max-w-44')
  })

  it('truncates a long error code so the status column stays narrow', () => {
    routeMock.useLoaderData.mockReturnValue({
      data: {
        items: [{ ...item, status: 'denied', error_code: 'mcp_tool_not_granted' }],
        page: 1,
        page_size: 25,
        total: 1,
      },
    })

    render(<McpInvocationsPage />)

    const code = within(screen.getByTestId('mcp-invocations-table')).getByTitle(
      'mcp_tool_not_granted',
    )
    expect(code).toHaveClass('truncate')
    expect(code.parentElement).toHaveClass('max-w-32')
  })

  it('shows URL filters as chips and removes one without touching the others', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [item], page: 1, page_size: 25, total: 1 },
    })
    routeMock.useSearch.mockReturnValue({ request_id: 'req_1', status: 'policy_denied' })

    render(<McpInvocationsPage />)

    const chips = within(screen.getByTestId('mcp-invocation-filter-chips'))
    expect(chips.getByTestId('mcp-invocation-filter-request-id')).toHaveValue('req_1')
    expect(chips.getByTestId('mcp-invocation-filter-status')).toHaveTextContent('Policy denied')

    fireEvent.click(screen.getByRole('button', { name: 'Remove Status filter' }))

    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    expect(navigatedSearch(0, { request_id: 'req_1', status: 'policy_denied' })).toEqual(
      expect.objectContaining({ request_id: 'req_1', status: undefined }),
    )
  })

  it('applies a typed owner chip on Enter', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [item], page: 1, page_size: 25, total: 1 },
    })

    render(<McpInvocationsPage />)

    fireEvent.keyDown(
      within(screen.getByTestId('mcp-invocation-toolbar')).getByRole('button', {
        name: /^Filters/,
      }),
      { key: 'Enter' },
    )
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Team ID' }))
    const teamId = await screen.findByTestId('mcp-invocation-filter-team-id')
    fireEvent.change(teamId, { target: { value: ' team_1 ' } })
    fireEvent.keyDown(teamId, { key: 'Enter' })

    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    expect(navigatedSearch(0)).toEqual(expect.objectContaining({ team_id: 'team_1' }))
  })

  it('opens the detail when a row is clicked', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [item], page: 1, page_size: 25, total: 1 },
    })
    getObservabilityMcpInvocationDetailMock.mockResolvedValue({
      data: { invocation: item, payload: null },
    })

    render(<McpInvocationsPage />)
    fireEvent.click(screen.getByRole('row', { name: 'Inspect create_issue on github' }))

    await waitFor(() => {
      expect(screen.getByText('Payload state')).toBeInTheDocument()
    })
    expect(getObservabilityMcpInvocationDetailMock).toHaveBeenCalledWith({
      data: { invocationId: 'mcpinv_1' },
    })
  })

  it('loads detail and renders sanitized payloads', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [item], page: 1, page_size: 25, total: 1 },
    })
    getObservabilityMcpInvocationDetailMock.mockResolvedValue({
      data: {
        invocation: item,
        payload: {
          arguments_json: { owner: 'ahstn', repo: 'oceans-llm' },
          result_json: { number: 115 },
        },
      },
    })

    render(<McpInvocationsPage />)
    fireEvent.click(screen.getByRole('row', { name: 'Inspect create_issue on github' }))

    // The detail card opens before the detail request resolves, so wait for the loaded body.
    await waitFor(() => {
      expect(screen.getByText('Payload state')).toBeInTheDocument()
    })

    expect(screen.getByTestId('mcp-invocation-detail')).toBeInTheDocument()
    expect(screen.getByText('Invocation detail')).toBeInTheDocument()
    expect(screen.getByText(/"repo": "oceans-llm"/)).toBeInTheDocument()
    expect(screen.getByText(/"number": 115/)).toBeInTheDocument()
  })

  it('renders scalar JSON payloads instead of treating them as missing', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [item], page: 1, page_size: 25, total: 1 },
    })
    getObservabilityMcpInvocationDetailMock.mockResolvedValue({
      data: {
        invocation: item,
        payload: {
          arguments_json: false,
          result_json: 0,
        },
      },
    })

    render(<McpInvocationsPage />)
    fireEvent.click(screen.getByRole('row', { name: 'Inspect create_issue on github' }))

    // The detail card opens before the detail request resolves, so wait for the loaded body.
    await waitFor(() => {
      expect(screen.getByText('Payload state')).toBeInTheDocument()
    })

    expect(screen.getByText('false')).toBeInTheDocument()
    expect(screen.getByText('0')).toBeInTheDocument()
    expect(screen.queryByText('No payload stored')).not.toBeInTheDocument()
  })

  it('renders an explicit empty state', () => {
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [], page: 1, page_size: 25, total: 0 },
    })

    render(<McpInvocationsPage />)

    expect(screen.getByText('No MCP invocations found')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Clear filters' })).not.toBeInTheDocument()
  })

  it('offers to clear filters when nothing matches them', () => {
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [], page: 1, page_size: 25, total: 0 },
    })
    routeMock.useSearch.mockReturnValue({ status: 'timeout' })

    render(<McpInvocationsPage />)

    expect(screen.getByText('No MCP calls match the current filters.')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Clear filters' }))
    expect(navigateMock).toHaveBeenCalledTimes(1)
  })
})

function navigatedSearch(callIndex: number, previous: Record<string, unknown> = {}) {
  const search = navigateMock.mock.calls[callIndex][0].search as (
    previous: Record<string, unknown>,
  ) => Record<string, unknown>
  return search(previous)
}
