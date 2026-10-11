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
  Link: ({ children, to }: { children: React.ReactNode; to: string }) => (
    <a href={to}>{children}</a>
  ),
}))

vi.mock('@/server/admin-data.functions', () => ({
  getGuardrailDecisionPage: vi.fn(),
  getGuardrailPolicyView: vi.fn(),
}))

const policy = {
  enabled: true,
  mode: 'audit',
  packs: ['core.shell'],
  managed_checks: [],
  stream_buffer_bytes: 4_194_304,
  secret_redaction: { enabled: true, tiers: ['credentials'], disabled_rules: [] },
  scope: 'global',
}

function decision(id: string, overrides: Record<string, unknown> = {}) {
  return {
    decision_id: id,
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
    ...overrides,
  }
}

const loaderData = {
  policies: {
    default: policy,
    model_routes: { fast: { ...policy, mode: 'enforce', scope: 'model_route:fast' } },
    mcp_servers: {},
    managed_checks: [],
    built_in_packs: [{ id: 'core.shell', version: '1.0.0' }],
  },
  decisions: {
    items: [
      decision('d1'),
      decision('d2', {
        phase: 'model_response',
        action: 'deny',
        reason_code: 'secret_detected',
        occurred_at: '2026-08-22T12:00:01Z',
      }),
      decision('d3', {
        request_id: 'request-2',
        effective_scope: 'model_route:fast',
        action: 'allow',
        reason_code: 'no_match',
      }),
    ],
    page: 1,
    page_size: 100,
    total: 3,
  },
}

async function renderLab(view?: string) {
  routeMock.useSearch.mockReturnValue(view ? { view } : {})
  const { GuardrailsLabPage } = await import('@/routes/observability/guardrails-lab')
  render(<GuardrailsLabPage />)
}

describe('GuardrailsLabPage', () => {
  afterEach(cleanup)

  beforeEach(() => {
    navigateMock.mockReset()
    routeMock.useLoaderData.mockReturnValue(loaderData)
  })

  it.each([
    ['pulse', 'Action mix by phase'],
    ['trace', 'Request traces'],
    ['pipeline', 'Guardrail pipeline'],
    ['scopes', 'Decisions in this scope'],
  ])('renders the %s candidate without mutation controls', async (view, heading) => {
    await renderLab(view)

    expect(screen.getByText(heading)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /save|edit|update/i })).not.toBeInTheDocument()
  })

  it('switches candidates through route search and keeps filters', async () => {
    routeMock.useSearch.mockReturnValue({ action: 'deny' })
    const { GuardrailsLabPage } = await import('@/routes/observability/guardrails-lab')
    render(<GuardrailsLabPage />)

    fireEvent.click(screen.getByRole('radio', { name: 'Trace' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/guardrails-lab',
        search: { action: 'deny', view: 'trace' },
      })
    })
  })

  it('groups trace steps by request with the most severe outcome', async () => {
    await renderLab('trace')

    expect(screen.getByRole('button', { name: 'request-1' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'request-2' })).toBeInTheDocument()
    expect(screen.getAllByText('Deny').length).toBeGreaterThan(1)
  })

  it('applies action filters server-side from the shared toolbar', async () => {
    await renderLab('pipeline')

    fireEvent.click(screen.getByRole('radio', { name: 'Deny' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/guardrails-lab',
        search: { action: 'deny', view: 'pipeline' },
      })
    })
  })

  it('shows scope overrides against the global default', async () => {
    await renderLab('scopes')

    fireEvent.click(screen.getByRole('button', { name: /fast/ }))

    expect(screen.getByText('Overrides mode.')).toBeInTheDocument()
    expect(screen.getByText('no_match')).toBeInTheDocument()
  })

  it('opens decision detail from the data grid', async () => {
    await renderLab('pulse')

    fireEvent.click(screen.getByText('secret_detected'))

    expect(await screen.findByText('sha256:fixture')).toBeInTheDocument()
  })
})
