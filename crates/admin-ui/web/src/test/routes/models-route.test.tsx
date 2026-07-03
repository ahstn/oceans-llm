import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { TooltipProvider } from '@/components/ui/tooltip'
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

const navigateMock = vi.fn()
const getModelClientConfigsMock = vi.hoisted(() => vi.fn())

const routeMock = {
  useLoaderData: vi.fn(),
  useSearch: vi.fn(),
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => ({
    navigate: navigateMock,
  }),
  redirect: vi.fn(),
}))

vi.mock('@/server/admin-data.functions', () => ({
  getModels: vi.fn(),
  getModelClientConfigs: getModelClientConfigsMock,
  getAuthSession: vi.fn(),
}))

const modelPage: ModelPageView = {
  items: [
    {
      id: 'fast',
      resolved_model_key: 'fast',
      alias_of: null,
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
      tags: ['fast', 'cheap'],
      status: 'healthy',
      client_configurations: [],
    },
    {
      id: 'claude-sonnet',
      resolved_model_key: 'claude-sonnet',
      alias_of: null,
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
      tags: ['anthropic', 'reasoning'],
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
                'model = "claude-sonnet"\nmodel_reasoning_effort = "medium"\nmodel_provider = "oceans-llm"\n\n[model_providers.oceans-llm]\nname = "oceans-llm"\nbase_url = "http://127.0.0.1:3000/v1"\nenv_key = "OCEANS_LLM_API_KEY"\nenv_key_instructions = "Set OCEANS_LLM_API_KEY in your environment"\nrequires_openai_auth = false\nwire_api = "responses"\n\n[analytics]\nenabled = false\n\n[otel]\nlog_user_prompt = false\n',
            },
          ],
          notes: ['Add this provider configuration to user-level ~/.codex/config.toml.'],
        },
      ],
    },
    {
      id: 'backup-fast',
      resolved_model_key: 'backup-fast',
      alias_of: 'fast',
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
      tags: ['fast', 'fallback'],
      status: 'degraded',
      client_configurations: [],
    },
  ],
  page: 1,
  page_size: 30,
  total: 3,
}

describe('ModelsPage', () => {
  beforeEach(() => {
    cleanup()
    routeMock.useLoaderData.mockReset()
    routeMock.useSearch.mockReset()
    navigateMock.mockReset()
    getModelClientConfigsMock.mockReset()
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 30 })
  })

  it('renders dedicated mobile and desktop model layouts from the same payload', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    const { ModelsPage } = await import('@/routes/models')

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    expect(screen.getByTestId('models-mobile-list')).toBeInTheDocument()
    expect(screen.getByTestId('models-desktop-table')).toBeInTheDocument()
    expect(
      screen.getByText('Review routed models, upstream targets, and current health status.'),
    ).toBeInTheDocument()
  })

  it('renders the desktop table with the expected column order and stacked routing cells', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    const { ModelsPage } = await import('@/routes/models')

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
      'Model id',
      'Upstream model',
      'Provider',
      'Cost / 1M tokens',
      'Context window',
      'Capabilities',
      'Client config',
    ])

    const identityCell = screen.getAllByTestId('models-desktop-cell-backup-fast')[0]
    expect(within(identityCell).getByText('backup-fast')).toBeInTheDocument()
    expect(within(identityCell).getByLabelText('degraded')).toBeInTheDocument()
    expect(within(identityCell).getByText('alias → fast')).toBeInTheDocument()

    const backupRow = within(table).getByText('backup-fast').closest('tr')
    expect(backupRow).not.toBeNull()
    const backupCells = within(backupRow as HTMLElement).getAllByRole('cell')

    expect(
      within(backupCells[2] as HTMLElement).getByText('google/gemini-2.0-flash'),
    ).toBeInTheDocument()
    expect(within(backupCells[3] as HTMLElement).getByText('Google Vertex AI')).toBeInTheDocument()
    expect(within(backupCells[3] as HTMLElement).getByText('vertex-gemini')).toBeInTheDocument()
    expect(within(backupCells[4] as HTMLElement).getByText('Input')).toBeInTheDocument()
    expect(within(backupCells[4] as HTMLElement).getByText('Output')).toBeInTheDocument()
    expect(within(backupCells[5] as HTMLElement).getByText('Input')).toBeInTheDocument()
    expect(within(backupCells[5] as HTMLElement).getByText('Output')).toBeInTheDocument()
    expect(within(backupCells[6] as HTMLElement).getByText('Streaming')).toBeInTheDocument()
    expect(within(backupCells[6] as HTMLElement).getByText('Vision')).toBeInTheDocument()
    expect(within(backupCells[7] as HTMLElement).getByText('—')).toBeInTheDocument()
  })

  it('does not render the notes column in the desktop table', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    const { ModelsPage } = await import('@/routes/models')

    render(
      <TooltipProvider>
        <ModelsPage />
      </TooltipProvider>,
    )

    const table = screen.getAllByTestId('models-desktop-table')[0]

    expect(within(table).queryByText('Notes')).not.toBeInTheDocument()
    expect(within(table).queryByText('Gemini fallback on Vertex')).not.toBeInTheDocument()
  })

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

    const { ModelsPage } = await import('@/routes/models')

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
    expect(await screen.findByRole('dialog', { name: 'Client config' })).toBeInTheDocument()
    expect(screen.getByText('~/.config/opencode/opencode.json')).toBeInTheDocument()
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
    expect(screen.getByText(/model_reasoning_effort = "medium"/)).toBeInTheDocument()
    expect(screen.getByText(/\[model_providers.oceans-llm\]/)).toBeInTheDocument()
    expect(screen.getByText(/env_key_instructions = "Set OCEANS_LLM_API_KEY in your environment"/)).toBeInTheDocument()
    expect(screen.getByText(/\[analytics\]/)).toBeInTheDocument()
    expect(screen.getByText(/log_user_prompt = false/)).toBeInTheDocument()
    expect(screen.getByText(/wire_api = "responses"/)).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Copy TOML' }))
    expect(writeText).toHaveBeenLastCalledWith(
      'model = "claude-sonnet"\nmodel_reasoning_effort = "medium"\nmodel_provider = "oceans-llm"\n\n[model_providers.oceans-llm]\nname = "oceans-llm"\nbase_url = "http://127.0.0.1:3000/v1"\nenv_key = "OCEANS_LLM_API_KEY"\nenv_key_instructions = "Set OCEANS_LLM_API_KEY in your environment"\nrequires_openai_auth = false\nwire_api = "responses"\n\n[analytics]\nenabled = false\n\n[otel]\nlog_user_prompt = false\n',
    )
  })

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

    const { ModelsPage } = await import('@/routes/models')

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
    expect(within(dialog).getByText('fast')).toBeInTheDocument()
    expect(within(dialog).getByText('claude-sonnet')).toBeInTheDocument()
    expect(within(dialog).getByText(/oceans-llm-openai-compatible/)).toBeInTheDocument()

    fireEvent.click(screen.getByRole('radio', { name: 'Pi' }))
    fireEvent.click(screen.getByRole('button', { name: 'Copy JSON' }))
    expect(writeText).toHaveBeenCalledWith(
      '{\n  "providers": {\n    "oceans-llm-openai-compatible": {},\n    "oceans-llm-anthropic-messages": {}\n  }\n}',
    )

    fireEvent.click(screen.getByRole('radio', { name: 'Claude Code' }))
    expect(within(dialog).getByText(/claude-sonnet-4-6/)).toBeInTheDocument()
    expect(within(dialog).getAllByText('claude-sonnet')).toHaveLength(2)
  })

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

    const { ModelsPage } = await import('@/routes/models')

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
    expect(within(dialog).getByText('claude-sonnet')).toBeInTheDocument()
    expect(within(dialog).getByText('fast')).toBeInTheDocument()
  })
})
