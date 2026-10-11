import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const navigateMock = vi.fn()
const routeMock = { useLoaderData: vi.fn(), useSearch: vi.fn() }

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => ({ navigate: navigateMock }),
  Link: ({ children, to }: { children: React.ReactNode; to: string }) => (
    <a href={to}>{children}</a>
  ),
}))

vi.mock('@/server/admin-data.functions', () => ({ getGuardrailPolicyView: vi.fn() }))

const base = {
  enabled: true,
  mode: 'audit',
  packs: ['core.shell'],
  managed_checks: [],
  stream_buffer_bytes: 4_194_304,
  secret_redaction: { enabled: true, tiers: ['credentials'], disabled_rules: [] },
  scope: 'global',
}

const policies = {
  default: base,
  model_routes: {
    fast: { ...base, mode: 'deny', scope: 'model_route:fast' },
    slow: { ...base, scope: 'model_route:slow' },
  },
  mcp_servers: {},
  managed_checks: [
    {
      name: 'company-bedrock',
      kind: 'amazon_bedrock',
      failure_disposition: 'fail_open',
      max_content_bytes: 262_144,
      phases: ['prompt'],
      resource: 'gr',
      timeout_ms: 2000,
    },
  ],
  built_in_packs: [
    { id: 'core.shell', version: '1.0.0' },
    { id: 'core.git', version: '1.0.0' },
  ],
}

async function renderMock(mock: string) {
  routeMock.useSearch.mockReturnValue({ mock })
  const { GuardrailSettingsLabPage } = await import('@/routes/observability/guardrails-settings')
  render(<GuardrailSettingsLabPage />)
}

describe('GuardrailSettingsLabPage', () => {
  afterEach(cleanup)
  beforeEach(() => {
    navigateMock.mockReset()
    routeMock.useLoaderData.mockReturnValue({ policies })
  })

  it.each(['settings-dialog', 'scope-dialog', 'side-tabs', 'area-cards', 'scope-matrix'])(
    'renders the %s mock with all three areas',
    async (mock) => {
      await renderMock(mock)
      for (const label of ['Destructive actions', 'Secret redaction', 'Integrations']) {
        expect(screen.getAllByText(new RegExp(label)).length).toBeGreaterThan(0)
      }
      expect(screen.queryByRole('button', { name: /^save/i })).not.toBeInTheDocument()
    },
  )

  it('lets inherited override fields follow the default while keeping explicit ones', async () => {
    await renderMock('side-tabs')

    fireEvent.click(screen.getByRole('radio', { name: 'Deny' }))
    fireEvent.click(screen.getByRole('switch', { name: /core\.git/ }))

    const yaml = screen.getByRole('region', { name: 'yaml code' })
    expect(yaml).toHaveTextContent('mode: deny')
    expect(yaml).toHaveTextContent('packs: [core.shell, core.git]')
    // `fast` set deny itself, `slow` inherited audit: neither needs its own mode line now.
    expect(yaml).toHaveTextContent('fast: mode: deny')
    expect(yaml).toHaveTextContent('slow: {}')
  })

  it('opens an area dialog for a scope from the matrix', async () => {
    await renderMock('scope-matrix')

    fireEvent.click(screen.getByRole('button', { name: 'Edit integrations for fast' }))

    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByText('company-bedrock')).toBeInTheDocument()
  })
})
