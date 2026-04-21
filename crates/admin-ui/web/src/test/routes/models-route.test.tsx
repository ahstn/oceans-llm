import { render, screen, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

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
}))

vi.mock('@/server/admin-data.functions', () => ({
  getModels: vi.fn(),
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

    render(<ModelsPage />)

    expect(screen.getByTestId('models-mobile-list')).toBeInTheDocument()
    expect(screen.getByTestId('models-desktop-table')).toBeInTheDocument()
    expect(
      screen.getByText('Review routed models, upstream targets, and current health status.'),
    ).toBeInTheDocument()
  })

  it('removes the resolved column and keeps status inside the model identity cell', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: modelPage })

    const { ModelsPage } = await import('@/routes/models')

    render(<ModelsPage />)

    const table = screen.getAllByTestId('models-desktop-table')[0]

    expect(within(table).queryByText('Resolved')).not.toBeInTheDocument()
    expect(within(table).getByText('Model ID')).toBeInTheDocument()

    const identityCell = screen.getAllByTestId('models-desktop-cell-backup-fast')[0]
    expect(within(identityCell).getByText('backup-fast')).toBeInTheDocument()
    expect(within(identityCell).getByText('degraded')).toBeInTheDocument()
    expect(within(identityCell).getByText('alias → fast')).toBeInTheDocument()
  })
})
