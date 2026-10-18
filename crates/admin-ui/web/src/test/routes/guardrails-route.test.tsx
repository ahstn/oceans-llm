import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { GuardrailsPage } from '@/routes/observability/guardrails'

const navigateMock = vi.fn()
let routeSearch = ''
// Hoisted with the mocks: the route module reads it as it loads.
const routeMock = vi.hoisted(() => ({
  useLoaderData: vi.fn(),
  useSearch: vi.fn(),
}))

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => ({ navigate: navigateMock }),
  Link: ({ children, to }: { children: React.ReactNode; to: string }) => (
    <a href={to}>{children}</a>
  ),
  useRouterState: ({
    select,
  }: {
    select: (state: { location: { searchStr: string } }) => unknown
  }) => select({ location: { searchStr: routeSearch } }),
}))

vi.mock('@/server/admin-data.functions', () => ({
  getGuardrailDecisionPage: vi.fn(),
}))

const loaderData = {
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
    routeSearch = ''
    routeMock.useLoaderData.mockReturnValue(loaderData)
    routeMock.useSearch.mockReturnValue({})
  })

  function renderPage() {
    render(<GuardrailsPage />)
  }

  it('renders privacy-safe decisions without policies or mutation controls', () => {
    renderPage()

    expect(screen.getByRole('heading', { level: 1, name: 'Guardrails' })).toBeInTheDocument()
    expect(screen.queryByText('Effective policies')).not.toBeInTheDocument()
    expect(screen.getByText('Denied')).toBeInTheDocument()
    expect(screen.getByText(/shell\.recursive-delete/)).toBeInTheDocument()
    expect(screen.getByText('destructive_operation')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /save|edit|update/i })).not.toBeInTheDocument()
    expect(
      screen.getByText(/Raw prompts, commands, arguments, and results are not shown/),
    ).toBeInTheDocument()
  })

  it('applies privacy-safe decision filters through route search', async () => {
    renderPage()

    fireEvent.change(screen.getByLabelText('Request ID'), { target: { value: 'request-1' } })
    fireEvent.submit(screen.getByLabelText('Request ID').closest('form')!)

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/guardrails',
        search: { request_id: 'request-1' },
      })
    })
  })

  it('filters by an evaluator typed into the toolbar', async () => {
    renderPage()

    fireEvent.change(screen.getByLabelText('Evaluator'), { target: { value: 'bedrock' } })
    fireEvent.submit(screen.getByLabelText('Evaluator').closest('form')!)

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/guardrails',
        search: { evaluator: 'bedrock' },
      })
    })
  })

  it('filters by action from the toolbar', async () => {
    renderPage()

    fireEvent.click(screen.getByRole('radio', { name: 'Deny' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/guardrails',
        search: { action: 'deny' },
      })
    })
  })

  it('filters by phase from the action mix', async () => {
    renderPage()

    fireEvent.click(screen.getByRole('button', { name: /Prompt/ }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/guardrails',
        search: { phase: 'prompt' },
      })
    })
  })

  it('locks phase, evaluator, and text filters while a navigation is pending', async () => {
    navigateMock.mockReturnValue(new Promise(() => {}))
    renderPage()

    fireEvent.click(screen.getByRole('radio', { name: 'Deny' }))

    await waitFor(() => expect(screen.getByRole('button', { name: /Prompt/ })).toBeDisabled())
    expect(screen.getByLabelText('Request ID')).toBeDisabled()
    expect(screen.getByLabelText('Evaluator')).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'deterministic' }))
    expect(navigateMock).toHaveBeenCalledTimes(1)
  })

  it('filters by evaluator from a decision row', async () => {
    renderPage()

    fireEvent.click(screen.getByRole('button', { name: 'deterministic' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/guardrails',
        search: { evaluator: 'deterministic' },
      })
    })
  })

  it('renders an empty state when no decisions match', () => {
    routeMock.useLoaderData.mockReturnValue({
      ...loaderData,
      decisions: { items: [], page: 1, page_size: 25, total: 0 },
    })
    renderPage()

    expect(screen.getByText('No guardrail decisions')).toBeInTheDocument()
    expect(screen.getByText('No events match the current filters.')).toBeInTheDocument()
  })

  it('navigates through paginated decision events', async () => {
    routeMock.useLoaderData.mockReturnValue({
      ...loaderData,
      decisions: { ...loaderData.decisions, page: 2, page_size: 25, total: 60 },
    })
    routeMock.useSearch.mockReturnValue({ evaluator: 'deterministic', page: 2, page_size: 25 })
    renderPage()

    fireEvent.click(screen.getByRole('button', { name: 'Next' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/guardrails',
        search: { evaluator: 'deterministic', page: 3, page_size: 25 },
      })
    })
  })

  it('links Configure to the guardrail settings page', () => {
    renderPage()

    expect(screen.getByRole('link', { name: 'Configure' })).toHaveAttribute('href', '/guardrails')
  })
})
