import { render, screen, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { TooltipProvider } from '@/components/ui/tooltip'
import type { ModelPageView } from '@/types/api'

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
  redirect: vi.fn(),
}))

vi.mock('@/server/admin-data.functions', () => ({
  getModels: vi.fn(),
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
    },
  ],
  page: 1,
  page_size: 30,
  total: 2,
}

describe('ModelsPage', () => {
  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
    routeMock.useSearch.mockReset()
    navigateMock.mockReset()
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
      'Model ID',
      'Upstream Model',
      'Provider',
      'Cost / 1M Tokens',
      'Context Window',
      'Capabilities',
    ])

    const identityCell = screen.getAllByTestId('models-desktop-cell-backup-fast')[0]
    expect(within(identityCell).getByText('backup-fast')).toBeInTheDocument()
    expect(within(identityCell).getByLabelText('degraded')).toBeInTheDocument()
    expect(within(identityCell).getByText('alias → fast')).toBeInTheDocument()

    const backupRow = within(table).getByText('backup-fast').closest('tr')
    expect(backupRow).not.toBeNull()
    const backupCells = within(backupRow as HTMLElement).getAllByRole('cell')

    expect(within(backupCells[1] as HTMLElement).getByText('google/gemini-2.0-flash')).toBeInTheDocument()
    expect(within(backupCells[2] as HTMLElement).getByText('Google Vertex AI')).toBeInTheDocument()
    expect(within(backupCells[2] as HTMLElement).getByText('vertex-gemini')).toBeInTheDocument()
    expect(within(backupCells[3] as HTMLElement).getByText('Input')).toBeInTheDocument()
    expect(within(backupCells[3] as HTMLElement).getByText('Output')).toBeInTheDocument()
    expect(within(backupCells[4] as HTMLElement).getByText('Input')).toBeInTheDocument()
    expect(within(backupCells[4] as HTMLElement).getByText('Output')).toBeInTheDocument()
    expect(within(backupCells[5] as HTMLElement).getByText('Streaming')).toBeInTheDocument()
    expect(within(backupCells[5] as HTMLElement).getByText('Vision')).toBeInTheDocument()
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
})
