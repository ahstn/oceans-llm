import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const navigateMock = vi.fn()
const routeMock = {
  useLoaderData: vi.fn(),
  useSearch: vi.fn(),
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => ({ navigate: navigateMock }),
}))

vi.mock('@/server/admin-data.functions', () => ({
  getGuardrailDecisionPage: vi.fn(),
  getGuardrailPolicyView: vi.fn(),
}))

const loaderData = {
  policies: {
    default: {
      enabled: true,
      mode: 'audit',
      packs: ['core.shell'],
      managed_checks: ['bedrock-primary'],
      stream_buffer_bytes: 4_194_304,
      scope: 'global',
    },
    model_routes: {},
    mcp_servers: {},
    managed_checks: [],
    built_in_packs: [{ id: 'core.shell', version: '1.0.0' }],
  },
  decisions: {
    items: [
      {
        decision_id: '11111111-1111-4111-8111-111111111111',
        request_id: 'request-1',
        mcp_tool_invocation_id: null,
        phase: 'prompt',
        effective_scope: 'global',
        evaluator: 'deterministic',
        managed_service: null,
        pack_id: 'core.shell',
        rule_id: 'shell.recursive-delete',
        action: 'audit',
        reason_code: 'destructive_operation',
        latency_micros: 42,
        failure_disposition: null,
        transformed: false,
        content_hash: 'sha256:fixture',
        occurred_at: '2026-08-22T12:00:00Z',
      },
    ],
    page: 1,
    page_size: 25,
    total: 1,
  },
}

describe('GuardrailsPage', () => {
  afterEach(cleanup)

  beforeEach(() => {
    navigateMock.mockReset()
    routeMock.useLoaderData.mockReturnValue(loaderData)
    routeMock.useSearch.mockReturnValue({})
  })

  it('renders effective policy and privacy-safe decisions without mutation controls', async () => {
    const { GuardrailsPage } = await import('@/routes/observability/guardrails')
    render(<GuardrailsPage />)

    expect(screen.getByRole('heading', { level: 1, name: 'Guardrails' })).toBeInTheDocument()
    expect(screen.getByText('Global default')).toBeInTheDocument()
    expect(screen.getByText('core.shell')).toBeInTheDocument()
    expect(screen.getByText(/shell\.recursive-delete/)).toBeInTheDocument()
    expect(screen.getByText('destructive_operation')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /save|edit|update/i })).not.toBeInTheDocument()
    expect(screen.getByText(/Raw prompts, commands, arguments, and results are not shown/)).toBeInTheDocument()
  })

  it('applies privacy-safe decision filters through route search', async () => {
    const { GuardrailsPage } = await import('@/routes/observability/guardrails')
    render(<GuardrailsPage />)

    fireEvent.change(screen.getByLabelText('Request ID'), { target: { value: 'request-1' } })
    fireEvent.change(screen.getByLabelText('Evaluator'), { target: { value: 'deterministic' } })
    fireEvent.click(screen.getByRole('button', { name: 'Apply filters' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/guardrails',
        search: { request_id: 'request-1', evaluator: 'deterministic' },
      })
    })
  })

  it('renders an empty state when no decisions match', async () => {
    routeMock.useLoaderData.mockReturnValue({
      ...loaderData,
      decisions: { items: [], page: 1, page_size: 25, total: 0 },
    })
    const { GuardrailsPage } = await import('@/routes/observability/guardrails')
    render(<GuardrailsPage />)

    expect(screen.getByText('No guardrail decisions')).toBeInTheDocument()
    expect(screen.getByText('No events match the current filters.')).toBeInTheDocument()
  })

  it('navigates through paginated decision events', async () => {
    routeMock.useLoaderData.mockReturnValue({
      ...loaderData,
      decisions: { ...loaderData.decisions, page: 2, page_size: 25, total: 60 },
    })
    routeMock.useSearch.mockReturnValue({ evaluator: 'deterministic', page: 2, page_size: 25 })
    const { GuardrailsPage } = await import('@/routes/observability/guardrails')
    render(<GuardrailsPage />)

    fireEvent.click(screen.getByRole('link', { name: 'Go to next page' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/guardrails',
        search: { evaluator: 'deterministic', page: 3, page_size: 25 },
      })
    })
  })
})
