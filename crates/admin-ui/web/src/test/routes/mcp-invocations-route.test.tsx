import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { McpInvocationView } from '@/types/api'

const getObservabilityMcpInvocationDetailMock = vi.fn()
const navigateMock = vi.fn()

const routeMock = {
  useLoaderData: vi.fn(),
  useSearch: vi.fn(),
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => ({
    navigate: navigateMock,
  }),
}))

vi.mock('@/server/admin-data.functions', () => ({
  getMcpInvocations: vi.fn(),
  getObservabilityMcpInvocationDetail: (...args: unknown[]) =>
    getObservabilityMcpInvocationDetailMock(...args),
}))

const item: McpInvocationView = {
  mcp_tool_invocation_id: 'mcpinv_1',
  request_id: 'req_1',
  api_key_id: 'api_key_1',
  user_id: null,
  team_id: 'team_1',
  server_id: 'server_1',
  server_display_key: 'github',
  server_display_name: 'GitHub',
  tool_id: 'tool_1',
  tool_display_key: 'create_issue',
  tool_display_name: 'Create issue',
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

  it('renders filters and invocation records', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [item], page: 1, page_size: 25, total: 1 },
    })

    const { McpInvocationsPage } = await import('@/routes/observability/mcp-invocations')

    render(<McpInvocationsPage />)

    expect(screen.getByText('MCP Invocations')).toBeInTheDocument()
    expect(screen.getByTestId('mcp-filter-request-id')).toBeInTheDocument()
    expect(screen.getByTestId('mcp-filter-server')).toBeInTheDocument()
    expect(screen.getByTestId('mcp-filter-tool')).toBeInTheDocument()
    expect(screen.getByTestId('mcp-invocations-table')).toBeInTheDocument()
    expect(screen.getByText('github')).toBeInTheDocument()
    expect(screen.getByText('create_issue')).toBeInTheDocument()
    expect(screen.getByText('allowed')).toBeInTheDocument()
    expect(screen.getByText('result truncated')).toBeInTheDocument()
  })

  it('applies request, server, tool, and owner-context filters through route search params', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [item], page: 1, page_size: 25, total: 1 },
    })

    const { McpInvocationsPage } = await import('@/routes/observability/mcp-invocations')

    render(<McpInvocationsPage />)

    fireEvent.change(screen.getByTestId('mcp-filter-request-id'), { target: { value: 'req_1' } })
    fireEvent.change(screen.getByTestId('mcp-filter-server'), { target: { value: 'github' } })
    fireEvent.change(screen.getByTestId('mcp-filter-tool'), { target: { value: 'create_issue' } })
    fireEvent.change(screen.getByTestId('mcp-filter-api-key'), { target: { value: 'api_key_1' } })
    fireEvent.change(screen.getByTestId('mcp-filter-team'), { target: { value: 'team_1' } })
    fireEvent.click(screen.getByRole('button', { name: 'Apply Filters' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/mcp-invocations',
        search: {
          request_id: 'req_1',
          server_display_key: 'github',
          tool_display_key: 'create_issue',
          api_key_id: 'api_key_1',
          user_id: undefined,
          team_id: 'team_1',
          status: undefined,
          policy_result: undefined,
          occurred_at_start: undefined,
          occurred_at_end: undefined,
        },
      })
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

    const { McpInvocationsPage } = await import('@/routes/observability/mcp-invocations')

    render(<McpInvocationsPage />)
    fireEvent.click(screen.getByRole('button', { name: 'Inspect' }))

    await waitFor(() => {
      expect(screen.getByTestId('mcp-invocation-detail')).toBeInTheDocument()
    })

    expect(screen.getByText('MCP Invocation Detail')).toBeInTheDocument()
    expect(screen.getByText('Payload State')).toBeInTheDocument()
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

    const { McpInvocationsPage } = await import('@/routes/observability/mcp-invocations')

    render(<McpInvocationsPage />)
    fireEvent.click(screen.getByRole('button', { name: 'Inspect' }))

    await waitFor(() => {
      expect(screen.getByTestId('mcp-invocation-detail')).toBeInTheDocument()
    })

    expect(screen.getByText('false')).toBeInTheDocument()
    expect(screen.getByText('0')).toBeInTheDocument()
    expect(screen.queryByText('No payload stored')).not.toBeInTheDocument()
  })

  it('renders an explicit empty state', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [], page: 1, page_size: 25, total: 0 },
    })

    const { McpInvocationsPage } = await import('@/routes/observability/mcp-invocations')

    render(<McpInvocationsPage />)

    expect(screen.getByText('No MCP invocations found')).toBeInTheDocument()
  })
})
