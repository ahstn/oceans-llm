import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { TooltipProvider } from '@/components/ui/tooltip'
import { ModelsPage } from '@/routes/models'
import type { ModelPageView } from '@/types/api'

type ClientConfigSetup = ModelPageView['items'][number]['client_configurations'][number]['setup']

const opencodeSetup = (): ClientConfigSetup => [
  {
    label: 'Configuration',
    value: '~/.config/opencode/opencode.json',
    href: null,
  },
  {
    label: 'API key',
    value: 'Set OCEANS_LLM_API_KEY to a gateway API key before using this OpenCode configuration.',
    href: null,
  },
  {
    label: 'Docs',
    value: 'https://opencode.ai/docs/config/',
    href: 'https://opencode.ai/docs/config/',
  },
]

const piSetup = (): ClientConfigSetup => [
  {
    label: 'Configuration',
    value:
      'Use ~/.pi/agent/models.json for this provider configuration; use ~/.pi/agent/settings.json or .pi/settings.json for Pi settings.',
    href: null,
  },
  {
    label: 'API key',
    value: 'Set OCEANS_LLM_API_KEY to a gateway API key before using this Pi configuration.',
    href: null,
  },
  {
    label: 'Docs',
    value: 'https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/settings.md',
    href: 'https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/settings.md',
  },
]

const claudeCodeSetup = (): ClientConfigSetup => [
  {
    label: 'Configuration',
    value:
      '~/.claude/settings.json for user configuration; .claude/settings.json for project configuration.',
    href: null,
  },
  {
    label: 'API key',
    value:
      'Replace <gateway api token> with a gateway API key before using this Claude Code configuration.',
    href: null,
  },
  {
    label: 'Docs',
    value: 'https://code.claude.com/docs/en/settings',
    href: 'https://code.claude.com/docs/en/settings',
  },
]

const codexSetup = (): ClientConfigSetup => [
  {
    label: 'Configuration',
    value: '~/.codex/config.toml',
    href: null,
  },
  {
    label: 'API key',
    value: 'Set OCEANS_LLM_API_KEY to a gateway API key before using this Codex configuration.',
    href: null,
  },
  {
    label: 'Docs',
    value: 'https://developers.openai.com/codex/config-reference',
    href: 'https://developers.openai.com/codex/config-reference',
  },
]

const navigateMock = vi.hoisted(() => vi.fn())
const invalidateMock = vi.hoisted(() => vi.fn())
const getModelsMock = vi.hoisted(() => vi.fn())
const getModelClientConfigsMock = vi.hoisted(() => vi.fn())
const refreshModelPricingMock = vi.hoisted(() => vi.fn())

const routeConfig = vi.hoisted(() => ({
  loader: undefined as ((input: { deps: Record<string, unknown> }) => Promise<unknown>) | undefined,
  validateSearch: undefined as
    | ((search: Record<string, unknown>) => Record<string, unknown>)
    | undefined,
}))

const routeMock = vi.hoisted(() => ({
  useLoaderData: vi.fn(),
  useRouteContext: vi.fn(),
  useLocationSearch: vi.fn(),
  routerStatus: vi.fn(),
}))

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => (config: typeof routeConfig) => {
    routeConfig.loader = config.loader
    routeConfig.validateSearch = config.validateSearch
    return routeMock
  },
  useLocation: ({
    select,
  }: {
    select: (location: { search: Record<string, unknown> }) => unknown
  }) => select({ search: routeMock.useLocationSearch() }),
  useRouterState: ({ select }: { select: (state: { status: string }) => unknown }) =>
    select({ status: routeMock.routerStatus() }),
  useRouter: () => ({
    navigate: navigateMock,
    invalidate: invalidateMock,
  }),
  redirect: vi.fn(),
}))

vi.mock('@/server/admin-data.functions', () => ({
  getModels: getModelsMock,
  getModelClientConfigs: getModelClientConfigsMock,
  refreshModelPricing: refreshModelPricingMock,
  getAuthSession: vi.fn(),
}))

const modelPage: ModelPageView = {
  items: [
    {
      id: 'fast',
      resolved_model_key: 'fast',
      alias_of: null,
      aliases: ['backup-fast'],
      description: 'Gemini via OpenRouter',
      provider_key: 'openrouter',
      provider_label: 'OpenRouter',
      provider_icon_key: 'openrouter',
      upstream_model: 'google/gemini-2.0-flash',
      model_icon_key: 'gemini',
      input_cost_per_million_tokens_usd_10000: 3_000,
      output_cost_per_million_tokens_usd_10000: 25_000,
      cache_read_cost_per_million_tokens_usd_10000: null,
      context_window_tokens: 1_048_576,
      input_window_tokens: null,
      output_window_tokens: 65_536,
      supports_streaming: true,
      supports_vision: true,
      supports_tool_calling: true,
      supports_structured_output: true,
      supports_attachments: true,
      benchmark_scores: [
        {
          metric_key: 'artificial_analysis_intelligence_index',
          label: 'Artificial Analysis Intelligence Index',
          value: 39,
          source: 'artificial_analysis',
          source_model_id: 'google/gemini-2.0-flash',
          source_url: 'https://openrouter.ai/google/gemini-2.0-flash',
          match_kind: 'derived',
          updated_at: '2026-09-24T00:00:00Z',
        },
      ],
      supports_decisions: true,
      tags: ['fast', 'cheap'],
      allowlist: null,
      status: 'healthy',
      client_configurations: [],
    },
    {
      id: 'claude-sonnet',
      resolved_model_key: 'claude-sonnet',
      alias_of: null,
      aliases: [],
      description: 'Claude Sonnet via Anthropic',
      provider_key: 'anthropic-prod',
      provider_label: 'Anthropic',
      provider_icon_key: 'anthropic',
      upstream_model: 'anthropic/claude-sonnet-4-6',
      model_icon_key: 'claude',
      input_cost_per_million_tokens_usd_10000: 30_000,
      output_cost_per_million_tokens_usd_10000: 150_000,
      cache_read_cost_per_million_tokens_usd_10000: 3_000,
      context_window_tokens: 200_000,
      input_window_tokens: null,
      output_window_tokens: 64_000,
      supports_streaming: true,
      supports_vision: false,
      supports_tool_calling: true,
      supports_structured_output: true,
      supports_attachments: false,
      benchmark_scores: [],
      supports_decisions: false,
      tags: ['anthropic', 'reasoning'],
      allowlist: {
        users: ['alice@example.com', 'bob@example.com'],
        teams: ['platform'],
      },
      status: 'healthy',
      client_configurations: [
        {
          key: 'opencode',
          label: 'OpenCode',
          model_ids: ['claude-sonnet'],
          setup: opencodeSetup(),
          blocks: [
            {
              label: 'opencode.json',
              filename: 'opencode.json',
              content: '{\n  "provider": "opencode"\n}',
            },
          ],
          notes: [],
        },
        {
          key: 'pi',
          label: 'Pi',
          model_ids: ['claude-sonnet'],
          setup: piSetup(),
          blocks: [
            {
              label: 'models.json',
              filename: 'models.json',
              content: '{\n  "provider": "pi"\n}',
            },
          ],
          notes: ['Manual note'],
        },
        {
          key: 'claude-code',
          label: 'Claude Code',
          model_ids: ['claude-sonnet'],
          setup: claudeCodeSetup(),
          blocks: [
            {
              label: 'Gateway model settings',
              filename: 'settings.json',
              content:
                '{\n  "$schema": "https://json.schemastore.org/claude-code-settings.json",\n  "env": {\n    "ANTHROPIC_MODEL": "claude-sonnet"\n  }\n}',
            },
            {
              label: 'Lower token usage settings',
              filename: 'settings.json',
              content:
                '{\n  "$schema": "https://json.schemastore.org/claude-code-settings.json",\n  "env": {\n    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "200000"\n  }\n}',
            },
          ],
          notes: [],
        },
        {
          key: 'codex',
          label: 'Codex',
          model_ids: ['claude-sonnet'],
          setup: codexSetup(),
          blocks: [
            {
              label: 'config.toml',
              filename: 'config.toml',
              content:
                'model = "claude-sonnet"\nmodel_provider = "oceans-llm"\n\n[model_providers.oceans-llm]\nname = "oceans-llm"\nbase_url = "http://127.0.0.1:3000/v1"\nenv_key = "OCEANS_LLM_API_KEY"\nenv_key_instructions = "Set OCEANS_LLM_API_KEY in your environment"\nrequires_openai_auth = false\nwire_api = "responses"\n\n[analytics]\nenabled = false\n\n[otel]\nlog_user_prompt = false\n',
            },
          ],
          notes: ['Add this provider configuration to user-level ~/.codex/config.toml.'],
        },
      ],
    },
    {
      id: 'vertex-fast',
      resolved_model_key: 'vertex-fast',
      alias_of: null,
      aliases: [],
      description: 'Gemini fallback on Vertex',
      provider_key: 'vertex-gemini',
      provider_label: 'Google Vertex AI',
      provider_icon_key: 'vertexai',
      upstream_model: 'google/gemini-2.0-flash',
      model_icon_key: 'gemini',
      input_cost_per_million_tokens_usd_10000: 3_000,
      output_cost_per_million_tokens_usd_10000: 25_000,
      cache_read_cost_per_million_tokens_usd_10000: null,
      context_window_tokens: 1_048_576,
      input_window_tokens: null,
      output_window_tokens: 65_536,
      supports_streaming: true,
      supports_vision: true,
      supports_tool_calling: false,
      supports_structured_output: true,
      supports_attachments: true,
      benchmark_scores: [],
      supports_decisions: false,
      tags: ['fast', 'fallback'],
      allowlist: null,
      status: 'degraded',
      client_configurations: [],
    },
  ],
  page: 1,
  page_size: 30,
  total: 3,
}

beforeEach(() => {
  cleanup()
  routeMock.useLoaderData.mockReset()
  routeMock.useRouteContext.mockReset()
  routeMock.useRouteContext.mockReturnValue({
    session: {
      must_change_password: false,
      user: {
        id: 'admin_1',
        name: 'Admin User',
        email: 'admin@example.com',
        global_role: 'platform_admin',
      },
    },
  })
  routeMock.useLocationSearch.mockReset()
  routeMock.routerStatus.mockReset()
  routeMock.routerStatus.mockReturnValue('idle')
  navigateMock.mockReset()
  invalidateMock.mockReset()
  getModelsMock.mockReset()
  getModelClientConfigsMock.mockReset()
  refreshModelPricingMock.mockReset()
  routeMock.useLocationSearch.mockReturnValue({ page: 1, page_size: 30 })
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('ModelsPage layouts', () => {
  it('forwards the URL query and pagination while requesting direct models', async () => {
    getModelsMock.mockResolvedValue({ data: modelPage })

    const deps = routeConfig.validateSearch!({ page: '2', page_size: '50', q: '  fast  ' })
    const result = await routeConfig.loader!({ deps })

    expect(getModelsMock).toHaveBeenCalledExactlyOnceWith({
      data: { page: 2, page_size: 50, q: '  fast  ', include_aliases: false },
    })
    expect(result).toEqual({ data: modelPage })
  })

  it('keeps aliases in the direct model info instead of separate model rows', () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const table = screen.getByTestId('models-desktop-table')
    const mobileList = screen.getByTestId('models-mobile-list')
    expect(within(table).queryByText('backup-fast')).not.toBeInTheDocument()
    expect(within(mobileList).queryByText('backup-fast')).not.toBeInTheDocument()
    const fastRow = within(table).getByText('fast').closest('tr')!
    fireEvent.click(within(fastRow).getByRole('button', { name: 'Info' }))

    const dialog = screen.getByRole('dialog', { name: 'Model info' })
    expect(within(dialog).getByText('Aliases')).toBeVisible()
    expect(within(dialog).getByText('backup-fast')).toBeVisible()
  })

  it('renders dedicated mobile and desktop model layouts from the same payload', () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    expect(screen.getByTestId('models-mobile-list')).toBeInTheDocument()
    expect(screen.getByTestId('models-desktop-table')).toBeInTheDocument()
    expect(
      screen.getByText('Review the models that users can select and check their current status.'),
    ).toBeInTheDocument()
    expect(screen.getByText('Select models to create a configuration file.')).toBeInTheDocument()

    const clearButton = screen.getByRole('button', { name: 'Clear' })
    const generateConfigButton = screen.getByRole('button', { name: 'Generate config' })
    expect(clearButton).toBeDisabled()
    expect(generateConfigButton).toBeDisabled()
    expect(clearButton).toHaveAttribute('data-variant', 'outline')
    expect(generateConfigButton).toHaveAttribute('data-variant', 'outline')
  })

  it('renders the desktop table with the expected column order and stacked routing cells', () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const table = screen.getAllByTestId('models-desktop-table')[0]
    const headers = within(table)
      .getAllByRole('columnheader')
      .map((header) => header.textContent?.trim())

    expect(within(table).queryByText('Resolved')).not.toBeInTheDocument()
    expect(headers).toEqual([
      '',
      'Model ID',
      'Actions',
      'Provider & Model',
      'Cost / 1M tokens',
      'Intelligence Index',
      'Allow List',
    ])

    const identityCell = screen.getAllByTestId('models-desktop-cell-vertex-fast')[0]
    expect(within(identityCell).getByText('vertex-fast')).toBeInTheDocument()
    expect(within(identityCell).getByLabelText('degraded')).toBeInTheDocument()

    const vertexRow = within(table).getByText('vertex-fast').closest('tr')
    expect(vertexRow).not.toBeNull()
    const vertexCells = within(vertexRow as HTMLElement).getAllByRole('cell')

    const infoButton = within(vertexCells[2] as HTMLElement).getByRole('button', { name: 'Info' })
    expect(infoButton).toHaveAttribute('data-variant', 'outline')

    const claudeRow = within(table).getByText('claude-sonnet').closest('tr')
    expect(claudeRow).not.toBeNull()
    const configButton = within(claudeRow as HTMLElement).getByRole('button', {
      name: 'Generate client config for claude-sonnet',
    })
    expect(configButton).toHaveAttribute('data-variant', 'outline')
    expect(
      within(vertexCells[3] as HTMLElement).getByText('google/gemini-2.0-flash'),
    ).toBeInTheDocument()
    expect(within(vertexCells[3] as HTMLElement).getByText('Google Vertex AI')).toBeInTheDocument()
    expect(within(vertexCells[4] as HTMLElement).getByText('Input')).toBeInTheDocument()
    expect(within(vertexCells[4] as HTMLElement).getByText('Output')).toBeInTheDocument()
    expect(within(vertexCells[6] as HTMLElement).getByText('Unrestricted')).toBeInTheDocument()
  })
})

describe('ModelsPage allowlists', () => {
  it('renders model allowlists in the desktop table as read-only details', () => {
    const allowlistPage: ModelPageView = {
      ...modelPage,
      items: modelPage.items.map((model) => {
        if (model.id === 'fast') {
          return {
            ...model,
            allowlist: {
              users: ['solo@example.com'],
              teams: [],
            },
          }
        }
        if (model.id === 'vertex-fast') {
          return {
            ...model,
            allowlist: {
              users: [],
              teams: ['platform', 'research'],
            },
          }
        }
        return model
      }),
    }
    routeMock.useLoaderData.mockReturnValue({ data: allowlistPage })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const table = screen.getAllByTestId('models-desktop-table')[0]
    expect(within(table).getByRole('columnheader', { name: 'Allow List' })).toBeInTheDocument()

    const fastRow = within(table).getByText('fast').closest('tr')
    expect(fastRow).not.toBeNull()
    const fastAllowlistCell = within(fastRow as HTMLElement).getAllByRole('cell')[6] as HTMLElement
    expect(within(fastAllowlistCell).getByText('Restricted')).toBeInTheDocument()
    expect(within(fastAllowlistCell).getByText('1 User')).toBeInTheDocument()
    expect(within(fastAllowlistCell).queryByText(/Teams?/)).not.toBeInTheDocument()

    const vertexRow = within(table).getByText('vertex-fast').closest('tr')
    expect(vertexRow).not.toBeNull()
    const vertexAllowlistCell = within(vertexRow as HTMLElement).getAllByRole(
      'cell',
    )[6] as HTMLElement
    expect(within(vertexAllowlistCell).getByText('Restricted')).toBeInTheDocument()
    expect(within(vertexAllowlistCell).getByText('2 Teams')).toBeInTheDocument()
    expect(within(vertexAllowlistCell).queryByText(/Users?/)).not.toBeInTheDocument()

    const claudeRow = within(table).getByText('claude-sonnet').closest('tr')
    expect(claudeRow).not.toBeNull()
    const claudeAllowlistCell = within(claudeRow as HTMLElement).getAllByRole(
      'cell',
    )[6] as HTMLElement
    expect(within(claudeAllowlistCell).getByText('Restricted')).toBeInTheDocument()
    expect(within(claudeAllowlistCell).getByText('2 Users')).toBeInTheDocument()
    expect(within(claudeAllowlistCell).getByText('1 Team')).toBeInTheDocument()
    expect(within(claudeAllowlistCell).queryByText('alice@example.com')).not.toBeInTheDocument()
    expect(within(claudeAllowlistCell).queryByText('bob@example.com')).not.toBeInTheDocument()
    expect(within(claudeAllowlistCell).queryByText('platform')).not.toBeInTheDocument()

    for (const allowlistCell of [fastAllowlistCell, vertexAllowlistCell, claudeAllowlistCell]) {
      expect(within(allowlistCell).queryByRole('button')).not.toBeInTheDocument()
      expect(within(allowlistCell).queryByRole('link')).not.toBeInTheDocument()
      expect(within(allowlistCell).queryByRole('checkbox')).not.toBeInTheDocument()
      expect(within(allowlistCell).queryByRole('textbox')).not.toBeInTheDocument()
    }
  })

  it('renders model allowlists in mobile cards as read-only details', () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const mobileList = screen.getByTestId('models-mobile-list')
    const claudeCard = within(mobileList)
      .getByRole('heading', { name: 'claude-sonnet' })
      .closest('[data-slot="card"]')
    expect(claudeCard).not.toBeNull()
    const claudeAllowlist = within(claudeCard as HTMLElement)
      .getByText('Model allowlist')
      .closest('div')
    expect(claudeAllowlist).not.toBeNull()
    expect(within(claudeAllowlist as HTMLElement).getByText('Users')).toBeInTheDocument()
    expect(
      within(claudeAllowlist as HTMLElement).getByText('alice@example.com'),
    ).toBeInTheDocument()
    expect(within(claudeAllowlist as HTMLElement).getByText('bob@example.com')).toBeInTheDocument()
    expect(within(claudeAllowlist as HTMLElement).getByText('Teams')).toBeInTheDocument()
    expect(within(claudeAllowlist as HTMLElement).getByText('platform')).toBeInTheDocument()

    const fastCard = within(mobileList)
      .getByRole('heading', { name: 'fast' })
      .closest('[data-slot="card"]')
    expect(fastCard).not.toBeNull()
    const fastAllowlist = within(fastCard as HTMLElement)
      .getByText('Model allowlist')
      .closest('div')
    expect(fastAllowlist).not.toBeNull()
    expect(within(fastAllowlist as HTMLElement).getByText('Unrestricted')).toBeInTheDocument()

    for (const allowlistDetail of [claudeAllowlist, fastAllowlist]) {
      expect(within(allowlistDetail as HTMLElement).queryByRole('button')).not.toBeInTheDocument()
      expect(within(allowlistDetail as HTMLElement).queryByRole('link')).not.toBeInTheDocument()
      expect(within(allowlistDetail as HTMLElement).queryByRole('checkbox')).not.toBeInTheDocument()
      expect(within(allowlistDetail as HTMLElement).queryByRole('textbox')).not.toBeInTheDocument()
    }
  })
})

describe('ModelsPage table content', () => {
  it('does not render the notes column in the desktop table', () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const table = screen.getAllByTestId('models-desktop-table')[0]

    expect(within(table).queryByText('Notes')).not.toBeInTheDocument()
    expect(within(table).queryByText('Gemini fallback on Vertex')).not.toBeInTheDocument()
  })

  it('always shows Intelligence Index and keeps missing scores unknown on desktop and mobile', () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const table = screen.getAllByTestId('models-desktop-table')[0]
    expect(
      within(table).getByRole('columnheader', { name: /^Intelligence Index/ }),
    ).toBeInTheDocument()
    const fastRow = within(table).getByText('fast').closest('tr')
    expect(fastRow).not.toBeNull()
    expect(within(fastRow as HTMLElement).getByText('39')).toBeInTheDocument()
    const claudeRow = within(table).getByText('claude-sonnet').closest('tr')
    expect(claudeRow).not.toBeNull()
    expect(within(claudeRow as HTMLElement).getByText('—')).toBeInTheDocument()

    const mobileList = screen.getByTestId('models-mobile-list')
    const fastCard = within(mobileList)
      .getByRole('heading', { name: 'fast' })
      .closest('[data-slot="card"]')!
    expect(within(fastCard as HTMLElement).getByText('Intelligence Index')).toBeVisible()
    expect(within(fastCard as HTMLElement).getByText('39')).toBeVisible()
    const claudeCard = within(mobileList)
      .getByRole('heading', { name: 'claude-sonnet' })
      .closest('[data-slot="card"]')!
    const mobileScore = within(claudeCard as HTMLElement)
      .getByText('Intelligence Index')
      .closest('div')!
    expect(within(mobileScore).getByText('—')).toBeVisible()
    expect(within(mobileScore).queryByText('0')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Columns' }))
    expect(screen.queryByRole('checkbox', { name: /^Intelligence/ })).not.toBeInTheDocument()
  })

  it('explains the Intelligence Index source when its help button receives focus', async () => {
    vi.stubGlobal(
      'ResizeObserver',
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    )
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })
    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const table = screen.getByTestId('models-desktop-table')
    const help = within(table).getByRole('button', { name: 'About Intelligence Index' })
    fireEvent.focus(help)
    expect(await screen.findByRole('tooltip')).toHaveTextContent(
      'Artificial Analysis Intelligence Index, retrieved via OpenRouter.',
    )
  })

  it('shows benchmark provenance and visible Artificial Analysis attribution', () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const table = screen.getAllByTestId('models-desktop-table')[0]
    const fastRow = within(table).getByText('fast').closest('tr')
    expect(fastRow).not.toBeNull()
    fireEvent.click(within(fastRow as HTMLElement).getByRole('button', { name: 'Info' }))
    const dialog = screen.getByRole('dialog', { name: 'Model info' })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Benchmarks' }))

    expect(within(dialog).getByRole('heading', { name: 'Benchmarks' })).toBeInTheDocument()
    expect(within(dialog).getByText('Artificial Analysis Intelligence Index')).toBeInTheDocument()
    expect(within(dialog).getByText('39')).toBeInTheDocument()
    expect(within(dialog).getByText(/Matched from upstream model · Updated/)).toBeInTheDocument()
    expect(
      within(dialog).getByRole('link', { name: 'google/gemini-2.0-flash on OpenRouter' }),
    ).toHaveAttribute('href', 'https://openrouter.ai/google/gemini-2.0-flash')
    expect(within(dialog).getByRole('link', { name: 'Artificial Analysis' })).toHaveAttribute(
      'href',
      'https://artificialanalysis.ai/',
    )
  })

  it('opens model info from the mobile model card', () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const mobileList = screen.getByTestId('models-mobile-list')
    fireEvent.click(within(mobileList).getByRole('button', { name: 'Model info for fast' }))

    expect(screen.getByRole('dialog', { name: 'Model info' })).toBeInTheDocument()
  })

  it('returns keyboard focus to the model Info button after closing', async () => {
    // Focusing the Info button opens its Radix tooltip, which observes its anchor size.
    vi.stubGlobal(
      'ResizeObserver',
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    )
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })
    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const row = screen.getByTestId('models-desktop-cell-fast').closest('tr') as HTMLElement
    const trigger = within(row).getByRole('button', { name: 'Info' })
    trigger.focus()
    fireEvent.click(trigger)
    const dialog = screen.getByRole('dialog', { name: 'Model info' })
    fireEvent.keyDown(dialog, { key: 'Escape' })

    await waitFor(() =>
      expect(screen.queryByRole('dialog', { name: 'Model info' })).not.toBeInTheDocument(),
    )
    await waitFor(() => expect(trigger).toHaveFocus())
  })

  it('shows Artificial Analysis attribution below the model list', () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const attribution = screen.getByText(/Benchmark scores by/)
    expect(attribution).toHaveClass('text-muted-foreground', 'text-right')
    expect(within(attribution).getByRole('link', { name: 'Artificial Analysis' })).toHaveAttribute(
      'href',
      'https://artificialanalysis.ai/',
    )
    expect(within(attribution).getByRole('link', { name: 'OpenRouter' })).toHaveAttribute(
      'href',
      'https://openrouter.ai/',
    )
  })

  it('renders a Decisions badge only for decisions-capable models', () => {
    const claude = modelPage.items[1]
    expect(claude).toBeDefined()
    const page: ModelPageView = {
      ...modelPage,
      items: [
        { ...(modelPage.items[0] as ModelPageView['items'][number]) },
        { ...(claude as ModelPageView['items'][number]), supports_decisions: true },
      ],
    }
    routeMock.useLoaderData.mockReturnValue({ data: page })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const mobileList = screen.getByTestId('models-mobile-list')
    const decisionsBadges = within(mobileList).getAllByText('Decisions')
    expect(decisionsBadges.length).toBeGreaterThan(0)
  })
})

describe('ModelsPage client configuration', () => {
  it('opens client config dialog, switches tabs, and copies active config blocks', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.assign(navigator, {
      clipboard: { writeText },
    })
    getModelClientConfigsMock.mockResolvedValue({
      data: { client_configurations: modelPage.items[1]?.client_configurations ?? [] },
      meta: {},
    })
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const table = screen.getAllByTestId('models-desktop-table')[0]
    const claudeRow = within(table).getByText('claude-sonnet').closest('tr')
    expect(claudeRow).not.toBeNull()

    fireEvent.click(
      within(claudeRow as HTMLElement).getByRole('button', {
        name: /Generate client config for claude-sonnet/i,
      }),
    )
    expect(getModelClientConfigsMock).toHaveBeenCalledWith({
      data: { model_keys: ['claude-sonnet'] },
    })
    const clientConfigDialog = await screen.findByRole('dialog', { name: 'Client config' })
    expect(clientConfigDialog).toBeInTheDocument()
    expect(clientConfigDialog).toHaveClass(
      'max-h-[min(880px,calc(100dvh-2rem))]',
      'max-w-[calc(100vw-2rem)]',
      'overflow-y-auto',
    )
    const harnessSelector = within(clientConfigDialog).getByRole('radiogroup', {
      name: 'Client config',
    })
    expect(harnessSelector).toHaveClass('min-w-0', 'max-w-full', 'flex-wrap')
    expect(harnessSelector).toHaveAttribute('data-spacing', '1')
    expect(clientConfigDialog.querySelectorAll('[data-agent-harness-icon]')).toHaveLength(4)
    expect(screen.getByText('~/.config/opencode/opencode.json')).toBeInTheDocument()
    expect(screen.getByText('Base URL')).toBeInTheDocument()
    expect(screen.getByText(/Base URL can change depending on API format/)).toBeInTheDocument()
    expect(screen.getByText('/v1')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'client harness configuration' })).toHaveAttribute(
      'href',
      'https://oceans-llm.com/configuration/client-harness-configuration.html',
    )
    expect(screen.getByRole('link', { name: 'https://opencode.ai/docs/config/' })).toHaveAttribute(
      'href',
      'https://opencode.ai/docs/config/',
    )
    expect(
      screen
        .getByText('~/.config/opencode/opencode.json')
        .compareDocumentPosition(screen.getByText(/"provider": "opencode"/)) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy()
    expect(screen.getByText('opencode.json')).toBeInTheDocument()
    expect(screen.getByText(/"provider": "opencode"/)).toBeInTheDocument()

    fireEvent.click(screen.getByRole('radio', { name: 'Pi' }))
    expect(screen.getByText(/~\/\.pi\/agent\/settings\.json/)).toBeInTheDocument()
    expect(screen.getByText(/\.pi\/settings\.json/)).toBeInTheDocument()
    expect(screen.getByText(/~\/\.pi\/agent\/models\.json/)).toBeInTheDocument()
    expect(
      screen.getByRole('link', {
        name: 'https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/settings.md',
      }),
    ).toHaveAttribute(
      'href',
      'https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/settings.md',
    )
    expect(screen.getByText('models.json')).toBeInTheDocument()
    expect(screen.getByText(/"provider": "pi"/)).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Copy JSON' }))
    expect(writeText).toHaveBeenCalledWith('{\n  "provider": "pi"\n}')
    expect(writeText).not.toHaveBeenCalledWith(expect.stringContaining('~/.pi/agent/settings.json'))

    fireEvent.click(screen.getByRole('radio', { name: 'Claude Code' }))
    expect(screen.getByText(/~\/\.claude\/settings\.json/)).toBeInTheDocument()
    expect(screen.getByText(/Replace <gateway api token>/)).toBeInTheDocument()
    expect(
      screen.getByRole('link', { name: 'https://code.claude.com/docs/en/settings' }),
    ).toHaveAttribute('href', 'https://code.claude.com/docs/en/settings')
    expect(screen.getAllByText('settings.json')).toHaveLength(2)
    expect(screen.getByText('Gateway model settings')).toBeInTheDocument()
    expect(screen.getByText('Lower token usage settings')).toBeInTheDocument()
    expect(screen.getByText(/"ANTHROPIC_MODEL": "claude-sonnet"/)).toBeInTheDocument()
    expect(screen.getByText(/"CLAUDE_CODE_AUTO_COMPACT_WINDOW": "200000"/)).toBeInTheDocument()

    const copyButtons = screen.getAllByRole('button', { name: 'Copy JSON' })
    fireEvent.click(copyButtons[1] as HTMLElement)
    expect(writeText).toHaveBeenLastCalledWith(
      '{\n  "$schema": "https://json.schemastore.org/claude-code-settings.json",\n  "env": {\n    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "200000"\n  }\n}',
    )

    fireEvent.click(screen.getByRole('radio', { name: 'Codex' }))
    expect(screen.getByText('~/.codex/config.toml')).toBeInTheDocument()
    expect(
      screen.getByText(/Set OCEANS_LLM_API_KEY to a gateway API key before using this Codex/),
    ).toBeInTheDocument()
    expect(
      screen.getByRole('link', {
        name: 'https://developers.openai.com/codex/config-reference',
      }),
    ).toHaveAttribute('href', 'https://developers.openai.com/codex/config-reference')
    expect(screen.getByText('config.toml')).toBeInTheDocument()
    expect(screen.getByText(/model = "claude-sonnet"/)).toBeInTheDocument()
    expect(screen.queryByText(/model_reasoning_effort/)).not.toBeInTheDocument()
    expect(screen.getByText(/\[model_providers.oceans-llm\]/)).toBeInTheDocument()
    expect(
      screen.getByText(/env_key_instructions = "Set OCEANS_LLM_API_KEY in your environment"/),
    ).toBeInTheDocument()
    expect(screen.getByText(/\[analytics\]/)).toBeInTheDocument()
    expect(screen.getByText(/log_user_prompt = false/)).toBeInTheDocument()
    expect(screen.getByText(/wire_api = "responses"/)).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Copy TOML' }))
    expect(writeText).toHaveBeenLastCalledWith(
      'model = "claude-sonnet"\nmodel_provider = "oceans-llm"\n\n[model_providers.oceans-llm]\nname = "oceans-llm"\nbase_url = "http://127.0.0.1:3000/v1"\nenv_key = "OCEANS_LLM_API_KEY"\nenv_key_instructions = "Set OCEANS_LLM_API_KEY in your environment"\nrequires_openai_auth = false\nwire_api = "responses"\n\n[analytics]\nenabled = false\n\n[otel]\nlog_user_prompt = false\n',
    )
  })
})

describe('ModelsPage pricing refresh', () => {
  it('refreshes pricing and reloads model data from the toolbar', async () => {
    refreshModelPricingMock.mockResolvedValue({ data: { refreshed: true }, meta: {} })
    invalidateMock.mockResolvedValue(undefined)
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Refresh pricing' }))

    expect(refreshModelPricingMock).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(invalidateMock).toHaveBeenCalledTimes(1))
  })

  it('keeps a successful pricing refresh when model data reload fails', async () => {
    refreshModelPricingMock.mockResolvedValue({ data: { refreshed: true }, meta: {} })
    invalidateMock.mockRejectedValue(new Error('reload failed'))
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Refresh pricing' }))

    expect(refreshModelPricingMock).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(invalidateMock).toHaveBeenCalledTimes(1))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Refresh pricing' })).toBeEnabled()
    })
  })
})

describe('ModelsPage multi-model configuration', () => {
  it('selects multiple models and opens generated client config for the selected set', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.assign(navigator, {
      clipboard: { writeText },
    })
    const mixedPage: ModelPageView = {
      ...modelPage,
      items: modelPage.items.map((item) =>
        item.id === 'fast'
          ? {
              ...item,
              client_configurations: [
                {
                  key: 'opencode',
                  label: 'OpenCode',
                  model_ids: ['fast'],
                  setup: opencodeSetup(),
                  blocks: [
                    {
                      label: 'opencode.json',
                      filename: 'opencode.json',
                      content: '{\n  "provider": "fast-only"\n}',
                    },
                  ],
                  notes: [],
                },
              ],
            }
          : item,
      ),
    }
    const generatedConfigs = [
      {
        key: 'opencode',
        label: 'OpenCode',
        model_ids: ['fast', 'claude-sonnet'],
        setup: opencodeSetup(),
        blocks: [
          {
            label: 'opencode.json',
            filename: 'opencode.json',
            content:
              '{\n  "provider": {\n    "oceans-llm-openai-compatible": {},\n    "oceans-llm-anthropic-messages": {}\n  }\n}',
          },
        ],
        notes: [],
      },
      {
        key: 'pi',
        label: 'Pi',
        model_ids: ['fast', 'claude-sonnet'],
        setup: piSetup(),
        blocks: [
          {
            label: 'models.json',
            filename: 'models.json',
            content:
              '{\n  "providers": {\n    "oceans-llm-openai-compatible": {},\n    "oceans-llm-anthropic-messages": {}\n  }\n}',
          },
        ],
        notes: [],
      },
      {
        key: 'claude-code',
        label: 'Claude Code',
        model_ids: ['claude-sonnet'],
        setup: claudeCodeSetup(),
        blocks: [
          {
            label: 'Gateway model settings',
            filename: 'settings.json',
            content: '{\n  "modelOverrides": {\n    "claude-sonnet-4-6": "claude-sonnet"\n  }\n}',
          },
        ],
        notes: [],
      },
    ]
    getModelClientConfigsMock.mockResolvedValue({
      data: { client_configurations: generatedConfigs },
      meta: {},
    })
    routeMock.useLoaderData.mockReturnValue({ data: mixedPage })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const table = screen.getAllByTestId('models-desktop-table')[0]
    fireEvent.click(within(table).getByLabelText('Select model fast'))
    fireEvent.click(within(table).getByLabelText('Select model claude-sonnet'))
    expect(screen.getByText('2 selected for client config')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Generate config' }))
    expect(getModelClientConfigsMock).toHaveBeenCalledWith({
      data: { model_keys: ['fast', 'claude-sonnet'] },
    })
    const dialog = await screen.findByRole('dialog', { name: 'Client config' })
    expect(dialog).toBeInTheDocument()
    expect(within(dialog).getByText('2 selected models')).toBeInTheDocument()
    expect(within(dialog).queryByText('fast')).not.toBeInTheDocument()
    expect(within(dialog).queryByText('claude-sonnet')).not.toBeInTheDocument()
    expect(within(dialog).getByText(/oceans-llm-openai-compatible/)).toBeInTheDocument()

    fireEvent.click(screen.getByRole('radio', { name: 'Pi' }))
    fireEvent.click(screen.getByRole('button', { name: 'Copy JSON' }))
    expect(writeText).toHaveBeenCalledWith(
      '{\n  "providers": {\n    "oceans-llm-openai-compatible": {},\n    "oceans-llm-anthropic-messages": {}\n  }\n}',
    )

    fireEvent.click(screen.getByRole('radio', { name: 'Claude Code' }))
    const claudeCode = within(dialog).getByRole('region', { name: 'json code' })
    expect(claudeCode).toHaveTextContent('"claude-sonnet-4-6": "claude-sonnet"')
    expect(claudeCode).not.toHaveTextContent('fast')
    expect(claudeCode).toHaveStyle({ maxHeight: 'calc(10 * 1.5rem + 2rem)' })
  })
})

describe('ModelsPage search and pagination', () => {
  it('updates the latest URL query without trimming input and resets the page', () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })
    routeMock.useLocationSearch.mockReturnValue({ page: 2, page_size: 30, q: 'previous query' })
    const { rerender } = render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const search = screen.getByRole('searchbox', { name: 'Search models' })
    expect(search).toHaveValue('previous query')
    fireEvent.change(search, { target: { value: '  Anthropic ' } })

    const navigation = navigateMock.mock.lastCall![0]
    expect(navigation).toMatchObject({ to: '/models', replace: true, resetScroll: false })
    const latestSearch = { page: 7, page_size: 50, q: 'newer URL value' }
    expect(navigation.search(latestSearch)).toEqual({ page: 1, page_size: 50, q: '  Anthropic ' })

    routeMock.useLocationSearch.mockReturnValue(navigation.search(latestSearch))
    rerender(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )
    expect(search).toHaveValue('  Anthropic ')

    fireEvent.change(search, { target: { value: '' } })
    expect(navigateMock.mock.lastCall![0].search(latestSearch)).toEqual({
      page: 1,
      page_size: 50,
      q: undefined,
    })
  })

  it('preserves the latest query when paging and when changing rows per page', () => {
    const page: ModelPageView = { ...modelPage, items: [modelPage.items[0]], page: 2, page_size: 1 }
    routeMock.useLoaderData.mockReturnValue({ data: page })
    routeMock.useLocationSearch.mockReturnValue({ page: 2, page_size: 1, q: 'fast' })
    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const pagination = screen.getByRole('navigation', { name: 'Model pagination' })
    expect(within(pagination).getByRole('status')).toHaveTextContent('2–2 of 3')
    expect(within(pagination).getByText('Page 2 of 3')).toBeVisible()
    const latestSearch = { page: 2, page_size: 1, q: '  new query  ' }
    fireEvent.click(within(pagination).getByRole('button', { name: 'Next page' }))
    expect(navigateMock.mock.lastCall![0].search(latestSearch)).toEqual({
      page: 3,
      page_size: 1,
      q: '  new query  ',
    })
    expect(navigateMock.mock.lastCall![0]).toMatchObject({ resetScroll: false })

    fireEvent.click(within(pagination).getByRole('button', { name: 'Previous page' }))
    expect(navigateMock.mock.lastCall![0].search(latestSearch)).toEqual({
      page: 1,
      page_size: 1,
      q: '  new query  ',
    })

    const pageSize = within(pagination).getByRole('combobox', { name: 'Rows per page' })
    expect(pageSize).toHaveTextContent('1')
    fireEvent.click(pageSize)
    fireEvent.click(screen.getByRole('option', { name: '50', exact: true }))
    expect(navigateMock.mock.lastCall![0].search(latestSearch)).toEqual({
      page: 1,
      page_size: 50,
      q: '  new query  ',
    })
  })

  it('shows an empty search result with a zero range and disabled pagination', () => {
    routeMock.useLoaderData.mockReturnValue({ data: { ...modelPage, items: [], total: 0 } })
    routeMock.useLocationSearch.mockReturnValue({ page: 1, page_size: 30, q: 'missing-model' })
    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    expect(screen.getByRole('searchbox', { name: 'Search models' })).toHaveValue('missing-model')
    expect(screen.getByText('No models found')).toBeVisible()
    const pagination = screen.getByRole('navigation', { name: 'Model pagination' })
    expect(within(pagination).getByRole('status')).toHaveTextContent('0–0 of 0')
    expect(within(pagination).getByText('Page 1 of 1')).toBeVisible()
    expect(within(pagination).getByRole('button', { name: 'Previous page' })).toBeDisabled()
    expect(within(pagination).getByRole('button', { name: 'Next page' })).toBeDisabled()
  })

  it('blocks stale pagination while a new search page is pending but keeps search editable', () => {
    const stalePage: ModelPageView = { ...modelPage, page: 4, page_size: 10, total: 100 }
    routeMock.useLoaderData.mockReturnValue({ data: stalePage })
    routeMock.useLocationSearch.mockReturnValue({ page: 1, page_size: 10, q: 'new search' })
    routeMock.routerStatus.mockReturnValue('pending')
    const { rerender } = render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const pagination = screen.getByRole('navigation', { name: 'Model pagination' })
    const previous = within(pagination).getByRole('button', { name: 'Previous page' })
    const next = within(pagination).getByRole('button', { name: 'Next page' })
    const pageSize = within(pagination).getByRole('combobox', { name: 'Rows per page' })
    expect(pagination).toHaveAttribute('aria-busy', 'true')
    expect(within(pagination).getByText('Page 4 of 10')).toBeVisible()
    expect(previous).toBeDisabled()
    expect(next).toBeDisabled()
    expect(pageSize).toBeDisabled()
    fireEvent.click(next)
    expect(navigateMock).not.toHaveBeenCalled()

    const search = screen.getByRole('searchbox', { name: 'Search models' })
    expect(search).toBeEnabled()
    expect(search).toHaveValue('new search')
    fireEvent.change(search, { target: { value: 'new search text' } })
    expect(navigateMock.mock.lastCall![0].search({ page: 1, page_size: 10 })).toEqual({
      page: 1,
      page_size: 10,
      q: 'new search text',
    })

    routeMock.useLoaderData.mockReturnValue({ data: { ...stalePage, page: 1 } })
    routeMock.useLocationSearch.mockReturnValue({ page: 1, page_size: 10, q: 'new search text' })
    routeMock.routerStatus.mockReturnValue('idle')
    rerender(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    expect(pagination).toHaveAttribute('aria-busy', 'false')
    expect(within(pagination).getByText('Page 1 of 10')).toBeVisible()
    expect(next).toBeEnabled()
    expect(pageSize).toBeEnabled()
    expect(previous).toBeDisabled()
  })

  it('keeps a selected model available after filtering removes its row', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })
    getModelClientConfigsMock.mockResolvedValue({
      data: { client_configurations: modelPage.items[1].client_configurations },
    })
    const { rerender } = render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    fireEvent.click(screen.getByLabelText('Select model claude-sonnet'))
    fireEvent.change(screen.getByRole('searchbox', { name: 'Search models' }), {
      target: { value: 'no-match' },
    })
    const nextSearch = navigateMock.mock.lastCall![0].search({ page: 1, page_size: 30 })
    routeMock.useLocationSearch.mockReturnValue(nextSearch)
    routeMock.useLoaderData.mockReturnValue({ data: { ...modelPage, items: [], total: 0 } })
    rerender(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    expect(screen.getByText('No models found')).toBeVisible()
    expect(screen.getByText('1 selected for client config')).toBeVisible()
    expect(screen.queryByLabelText('Select model claude-sonnet')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Generate config' }))
    expect(getModelClientConfigsMock).toHaveBeenCalledExactlyOnceWith({
      data: { model_keys: ['claude-sonnet'] },
    })
    expect(await screen.findByRole('dialog', { name: 'Client config' })).toBeVisible()
  })
})

describe('ModelsPage paginated configuration', () => {
  it('keeps selected models available when generating after pagination', async () => {
    const configurableFast = {
      ...(modelPage.items[0] as ModelPageView['items'][number]),
      client_configurations: [
        {
          key: 'opencode',
          label: 'OpenCode',
          model_ids: ['fast'],
          setup: opencodeSetup(),
          blocks: [
            {
              label: 'opencode.json',
              filename: 'opencode.json',
              content: '{\n  "provider": "fast-only"\n}',
            },
          ],
          notes: [],
        },
      ],
    }
    const pageOne: ModelPageView = {
      ...modelPage,
      items: [modelPage.items[1] as ModelPageView['items'][number]],
      page: 1,
      page_size: 1,
      total: 2,
    }
    const pageTwo: ModelPageView = {
      ...modelPage,
      items: [configurableFast],
      page: 2,
      page_size: 1,
      total: 2,
    }
    getModelClientConfigsMock.mockResolvedValue({
      data: {
        client_configurations: [
          {
            key: 'opencode',
            label: 'OpenCode',
            model_ids: ['claude-sonnet', 'fast'],
            setup: opencodeSetup(),
            blocks: [
              {
                label: 'opencode.json',
                filename: 'opencode.json',
                content: '{\n  "provider": "mixed"\n}',
              },
            ],
            notes: [],
          },
        ],
      },
      meta: {},
    })
    routeMock.useLoaderData.mockReturnValue({ data: pageOne })

    const { rerender } = render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    fireEvent.click(screen.getByLabelText('Select model claude-sonnet'))
    routeMock.useLoaderData.mockReturnValue({ data: pageTwo })
    rerender(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )
    fireEvent.click(screen.getByLabelText('Select model fast'))

    expect(screen.getByText('2 selected for client config')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Generate config' }))

    expect(getModelClientConfigsMock).toHaveBeenCalledWith({
      data: { model_keys: ['claude-sonnet', 'fast'] },
    })
    const dialog = await screen.findByRole('dialog', { name: 'Client config' })
    expect(within(dialog).getByText('2 selected models')).toBeInTheDocument()
    expect(within(dialog).queryByText('claude-sonnet')).not.toBeInTheDocument()
    expect(within(dialog).queryByText('fast')).not.toBeInTheDocument()
  })
})

describe('ModelsPage permissions', () => {
  it('shows all models and client config actions without admin controls to regular users', () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })
    routeMock.useRouteContext.mockReturnValue({
      session: {
        must_change_password: false,
        user: {
          id: 'user_1',
          name: 'Regular User',
          email: 'user@example.com',
          global_role: 'user',
        },
      },
    })

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    expect(screen.getAllByText('fast').length).toBeGreaterThan(0)
    expect(screen.getAllByText('claude-sonnet').length).toBeGreaterThan(0)
    expect(screen.getByRole('button', { name: 'Generate config' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Refresh pricing' })).not.toBeInTheDocument()
    expect(screen.queryByText('Allow List')).not.toBeInTheDocument()
    expect(screen.queryByText('alice@example.com')).not.toBeInTheDocument()
    const table = screen.getByTestId('models-desktop-table')
    expect(within(table).getByRole('columnheader', { name: /^Intelligence Index/ })).toBeVisible()
    const fastRow = within(table).getByText('fast').closest('tr')!
    expect(within(fastRow).getByText('39')).toBeVisible()
  })
})
