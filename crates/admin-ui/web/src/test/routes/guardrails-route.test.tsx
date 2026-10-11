import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const navigateMock = vi.fn()
let routeSearch = ''
const routeMock = {
  useLoaderData: vi.fn(),
  useSearch: vi.fn(),
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => ({ navigate: navigateMock }),
  useRouterState: ({
    select,
  }: {
    select: (state: { location: { searchStr: string } }) => unknown
  }) => select({ location: { searchStr: routeSearch } }),
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
      secret_redaction: {
        enabled: true,
        tiers: ['credentials', 'provider_tokens'],
        disabled_rules: [],
      },
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
    routeSearch = ''
    routeMock.useLoaderData.mockReturnValue(loaderData)
    routeMock.useSearch.mockReturnValue({})
  })

  async function renderPage() {
    const { GuardrailsPage } = await import('@/routes/observability/guardrails')
    render(<GuardrailsPage />)
  }

  it('renders effective policy and privacy-safe decisions without mutation controls', async () => {
    await renderPage()

    expect(screen.getByRole('heading', { level: 1, name: 'Guardrails' })).toBeInTheDocument()
    expect(screen.getByText('Global default')).toBeInTheDocument()
    expect(screen.getByText('core.shell')).toBeInTheDocument()
    expect(screen.getByText('Redaction credentials, provider_tokens')).toBeInTheDocument()
    expect(screen.getByText(/shell\.recursive-delete/)).toBeInTheDocument()
    expect(screen.getByText('destructive_operation')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /save|edit|update/i })).not.toBeInTheDocument()
    expect(
      screen.getByText(/Raw prompts, commands, arguments, and results are not shown/),
    ).toBeInTheDocument()
  })

  it('applies privacy-safe decision filters through route search', async () => {
    await renderPage()

    fireEvent.change(screen.getByLabelText('Request ID'), { target: { value: 'request-1' } })
    fireEvent.submit(screen.getByLabelText('Request ID').closest('form')!)

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/guardrails',
        search: { request_id: 'request-1' },
      })
    })
  })

  it('filters by action and by phase from the action mix', async () => {
    await renderPage()

    fireEvent.click(screen.getByRole('radio', { name: 'Deny' }))
    fireEvent.click(screen.getByRole('button', { name: /Prompt/ }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/guardrails',
        search: { action: 'deny' },
      })
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/guardrails',
        search: { phase: 'prompt' },
      })
    })
  })

  it('filters by evaluator from a decision row', async () => {
    await renderPage()

    fireEvent.click(screen.getByRole('button', { name: 'deterministic' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/guardrails',
        search: { evaluator: 'deterministic' },
      })
    })
  })

  it('renders an empty state when no decisions match', async () => {
    routeMock.useLoaderData.mockReturnValue({
      ...loaderData,
      decisions: { items: [], page: 1, page_size: 25, total: 0 },
    })
    await renderPage()

    expect(screen.getByText('No guardrail decisions')).toBeInTheDocument()
    expect(screen.getByText('No events match the current filters.')).toBeInTheDocument()
  })

  it('navigates through paginated decision events', async () => {
    routeMock.useLoaderData.mockReturnValue({
      ...loaderData,
      decisions: { ...loaderData.decisions, page: 2, page_size: 25, total: 60 },
    })
    routeMock.useSearch.mockReturnValue({ evaluator: 'deterministic', page: 2, page_size: 25 })
    await renderPage()

    fireEvent.click(screen.getByRole('button', { name: 'Next' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/guardrails',
        search: { evaluator: 'deterministic', page: 3, page_size: 25 },
      })
    })
  })

  it('drafts configuration changes as YAML without saving them', async () => {
    await renderPage()

    fireEvent.click(screen.getByRole('button', { name: 'Configure' }))
    fireEvent.click(await screen.findByRole('radio', { name: 'Deny' }))

    const yaml = await screen.findByRole('region', { name: 'yaml code' })
    expect(yaml).toHaveTextContent('mode: deny')
    expect(yaml).toHaveTextContent('tiers: [credentials, provider_tokens]')
    expect(screen.getByRole('button', { name: 'Copy YAML' })).toBeInTheDocument()
    expect(navigateMock).not.toHaveBeenCalled()
  })
})
