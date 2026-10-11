import { useState } from 'react'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { toast } from 'sonner'

import { TooltipProvider } from '@/components/ui/tooltip'
import { ModelInfoDialog, type ModelInfoSectionKey } from '@/routes/-model-info-dialog'
import type { ModelView } from '@/types/api'

const model: ModelView = {
  id: 'gpt-6-sol',
  model_id: '10000000-0000-0000-0000-000000000001',
  resolved_model_key: 'gpt-6-sol',
  alias_of: null,
  aliases: [],
  description: 'Shared provider pool',
  tags: [],
  allowlist: null,
  status: 'healthy',
  provider_key: 'copilot-personal',
  provider_label: 'GitHub Copilot',
  provider_icon_key: null,
  upstream_model: 'gpt-6-sol-copilot',
  model_icon_key: 'openai',
  benchmark_scores: [],
  client_configurations: [],
  pricing_varies_by_route: false,
  routing: {
    strategy: 'round_robin',
    affinity: { idle_timeout_seconds: 3_600 },
  },
  routes: [
    {
      id: '20000000-0000-0000-0000-000000000001',
      provider_key: 'copilot-personal',
      provider_label: 'GitHub Copilot',
      provider_icon_key: 'openai',
      upstream_model: 'gpt-6-sol-copilot',
      priority: 0,
      weight: 1,
      enabled: true,
      provider_configured: true,
    },
    {
      id: '20000000-0000-0000-0000-000000000002',
      provider_key: 'openrouter-prod',
      provider_label: 'OpenRouter',
      provider_icon_key: 'openrouter',
      upstream_model: 'openai/gpt-6-sol',
      priority: 0,
      weight: 2,
      enabled: true,
      provider_configured: true,
    },
  ],
}

const routes = model.routes!

function renderDialog(
  selectedModel: ModelView | null = model,
  {
    initialSection = 'routing',
    showAccessDetails = true,
  }: {
    initialSection?: ModelInfoSectionKey
    showAccessDetails?: boolean
  } = {},
) {
  function DialogHarness() {
    const [activeSection, setActiveSection] = useState<ModelInfoSectionKey>(initialSection)

    return (
      <TooltipProvider>
        <ModelInfoDialog
          model={selectedModel}
          activeSection={activeSection}
          onActiveSectionChange={setActiveSection}
          onOpenChange={vi.fn()}
          showAccessDetails={showAccessDetails}
        />
      </TooltipProvider>
    )
  }

  return render(<DialogHarness />)
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('ModelInfoDialog routing', () => {
  it('shows every provider route and the configured session policy', () => {
    renderDialog()

    const dialog = screen.getByRole('dialog', { name: 'Model info' })
    expect(within(dialog).getByText('GitHub Copilot')).toBeVisible()
    expect(within(dialog).getByText('OpenRouter')).toBeVisible()
    expect(within(dialog).getByText('gpt-6-sol-copilot')).toBeVisible()
    expect(within(dialog).getByText('openai/gpt-6-sol')).toBeVisible()
    expect(within(dialog).queryByText(/^via GitHub Copilot$/)).not.toBeInTheDocument()
    expect(within(dialog).getByText('Round robin')).toBeVisible()
    expect(within(dialog).getByText('1 hour idle')).toBeVisible()
    expect(within(dialog).getAllByText('Enabled')).toHaveLength(2)
  })

  it('keeps route and model identifiers behind separate details controls', () => {
    renderDialog()

    const dialog = screen.getByRole('dialog', { name: 'Model info' })
    expect(within(dialog).queryByText(routes[0].id)).not.toBeInTheDocument()
    expect(within(dialog).queryByText(routes[1].id)).not.toBeInTheDocument()
    expect(within(dialog).queryByText(model.model_id)).not.toBeInTheDocument()
    expect(within(dialog).queryByText('openrouter-prod')).not.toBeInTheDocument()

    const details = within(dialog).getByRole('button', { name: 'Details for OpenRouter, route 2' })
    expect(details).toHaveAttribute('aria-expanded', 'false')
    fireEvent.click(details)
    expect(details).toHaveAttribute('aria-expanded', 'true')

    const route = within(dialog).getByTestId(`model-route-${routes[1].id}`)
    expect(within(route).getByText('Provider key')).toBeVisible()
    expect(within(route).getByText('openrouter-prod')).toBeVisible()
    expect(within(route).getByText('Route ID')).toBeVisible()
    expect(within(route).getByText(routes[1].id)).toBeVisible()
    expect(within(dialog).queryByText(routes[0].id)).not.toBeInTheDocument()
    expect(within(dialog).queryByText(model.model_id)).not.toBeInTheDocument()

    fireEvent.click(within(dialog).getByRole('button', { name: 'Technical details' }))
    expect(within(dialog).getByText(model.model_id)).toBeVisible()

    fireEvent.click(details)
    expect(within(dialog).queryByText(routes[1].id)).not.toBeInTheDocument()
  })

  it('gives repeated provider labels distinct names and complete accessible descriptions', () => {
    renderDialog({
      ...model,
      routes: [
        {
          ...routes[1],
          id: 'openrouter-primary',
          upstream_model: 'openai/gpt-6-sol-primary',
          priority: 0,
          weight: 3,
        },
        {
          ...routes[1],
          id: 'openrouter-secondary',
          upstream_model: 'openai/gpt-6-sol-secondary',
          priority: 2,
          weight: 0,
        },
      ],
    })

    const dialog = screen.getByRole('dialog', { name: 'Model info' })
    const primary = within(dialog).getByRole('button', { name: 'Details for OpenRouter, route 1' })
    const secondary = within(dialog).getByRole('button', {
      name: 'Details for OpenRouter, route 2',
    })
    expect(primary).toHaveAccessibleDescription(/OpenRouter/)
    expect(primary).toHaveAccessibleDescription(/openai\/gpt-6-sol-primary/)
    expect(primary).toHaveAccessibleDescription(/Enabled/)
    expect(primary).toHaveAccessibleDescription(/Priority 0.*Weight 3/)
    expect(secondary).toHaveAccessibleDescription(/OpenRouter/)
    expect(secondary).toHaveAccessibleDescription(/openai\/gpt-6-sol-secondary/)
    expect(secondary).toHaveAccessibleDescription(/Zero weight/)
    expect(secondary).toHaveAccessibleDescription(/Priority 2.*Weight 0/)
    expect(primary).not.toHaveAttribute(
      'aria-describedby',
      secondary.getAttribute('aria-describedby'),
    )

    fireEvent.click(secondary)
    expect(secondary).toHaveAttribute('aria-expanded', 'true')
    expect(primary).toHaveAttribute('aria-expanded', 'false')
  })

  it('distinguishes disabled, missing-provider, and zero-weight routes', () => {
    const unavailableRoutes = [
      { ...routes[0], id: 'disabled-route', enabled: false },
      { ...routes[1], id: 'missing-provider-route', provider_configured: false },
      { ...routes[1], id: 'zero-weight-route', weight: 0 },
    ]
    renderDialog({ ...model, routes: unavailableRoutes, status: 'degraded' })

    for (const [routeId, status] of [
      ['disabled-route', 'Disabled'],
      ['missing-provider-route', 'Missing provider'],
      ['zero-weight-route', 'Zero weight'],
    ]) {
      const route = screen.getByTestId(`model-route-${routeId}`)
      expect(within(route).getByText(status)).toBeVisible()
      expect(within(route).queryByText('Enabled')).not.toBeInTheDocument()
    }
  })

  it('uses the scalar fallback when route details are restricted', () => {
    renderDialog({ ...model, routes: null, routing: null }, { showAccessDetails: false })

    const dialog = screen.getByRole('dialog', { name: 'Model info' })
    expect(within(dialog).getByText('gpt-6-sol-copilot')).toBeVisible()
    expect(within(dialog).getAllByText('GitHub Copilot').length).toBeGreaterThan(0)
    expect(within(dialog).queryByText('openai/gpt-6-sol')).not.toBeInTheDocument()
    expect(within(dialog).queryByText('Round robin')).not.toBeInTheDocument()
    expect(within(dialog).queryByText('Weighted random')).not.toBeInTheDocument()
    expect(within(dialog).queryByText('Preferred')).not.toBeInTheDocument()
    expect(within(dialog).queryByText('1 hour idle')).not.toBeInTheDocument()
    expect(within(dialog).queryByRole('button', { name: /^Details for / })).not.toBeInTheDocument()
    expect(within(dialog).queryByRole('button', { name: 'Access' })).not.toBeInTheDocument()
  })

  it('shows an empty route pool without presenting the scalar provider as a route', () => {
    renderDialog({ ...model, routes: [], status: 'degraded' })

    const dialog = screen.getByRole('dialog', { name: 'Model info' })
    expect(within(dialog).getByText(/no .*routes/i)).toBeVisible()
    expect(within(dialog).queryByRole('button', { name: /^Details for / })).not.toBeInTheDocument()
    expect(within(dialog).queryByText('gpt-6-sol-copilot')).not.toBeInTheDocument()
  })

  it('keeps section navigation and long routing details inside the narrow dialog', () => {
    renderDialog(model, { initialSection: 'overview' })

    const dialog = screen.getByRole('dialog', { name: 'Model info' })
    expect(dialog).toHaveClass('sm:max-w-3xl', 'overflow-hidden')
    const layout = within(dialog).getByTestId('model-info-layout')
    expect(layout).toHaveClass('min-w-0', 'overflow-hidden')
    const content = within(dialog).getByTestId('model-info-content')
    expect(content).toHaveClass('min-w-0', 'overflow-y-auto')

    const navigation = within(dialog).getByRole('navigation', { name: 'Model info sections' })
    expect(navigation).toHaveClass('md:flex-col')
    const overview = within(navigation).getByRole('button', { name: 'Overview' })
    const routing = within(navigation).getByRole('button', { name: 'Routing' })
    expect(overview).toHaveAttribute('aria-current', 'page')
    expect(routing).toHaveAttribute('aria-controls', content.id)
    fireEvent.click(routing)
    expect(routing).toHaveAttribute('aria-current', 'page')
    expect(overview).not.toHaveAttribute('aria-current', 'page')
    expect(within(content).getByText('openai/gpt-6-sol')).toBeVisible()
  })

  it('does not open a dialog before a model is selected', () => {
    renderDialog(null)
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('shows the complete alias list separately from the resolved model', () => {
    renderDialog(
      {
        ...model,
        id: 'daily-driver',
        alias_of: 'coding-default',
        aliases: ['coding-default', 'gpt-6-sol', 'weekend-driver'],
        resolved_model_key: 'gpt-6-sol',
      },
      { initialSection: 'overview' },
    )

    const dialog = screen.getByRole('dialog', { name: 'Model info' })
    const aliasRow = within(dialog).getByText('Aliases').closest('div')!
    expect(within(aliasRow).getByText('coding-default')).toBeVisible()
    expect(within(aliasRow).getByText('gpt-6-sol')).toBeVisible()
    expect(within(aliasRow).getByText('weekend-driver')).toBeVisible()
    expect(within(aliasRow).queryByText('daily-driver')).not.toBeInTheDocument()

    const navigation = within(dialog).getByRole('navigation', { name: 'Model info sections' })
    fireEvent.click(within(navigation).getByRole('button', { name: 'Routing' }))
    fireEvent.click(within(dialog).getByRole('button', { name: 'Technical details' }))

    const resolvedRow = within(dialog).getByText('Resolved model').closest('div')!
    expect(within(resolvedRow).getByText('gpt-6-sol')).toBeVisible()
    expect(within(resolvedRow).queryByText('coding-default')).not.toBeInTheDocument()
  })

  it('copies the selected gateway ID instead of its resolved model', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    vi.stubGlobal('navigator', { clipboard: { writeText } })
    const success = vi.spyOn(toast, 'success').mockImplementation(() => 0)
    renderDialog(
      { ...model, id: 'daily-driver', alias_of: model.id },
      { initialSection: 'overview' },
    )

    fireEvent.click(screen.getByRole('button', { name: 'Copy model ID' }))

    await waitFor(() => expect(success).toHaveBeenCalledWith('Model ID copied'))
    expect(writeText).toHaveBeenCalledExactlyOnceWith('daily-driver')
  })

  it('reports a clipboard failure without claiming that the ID was copied', async () => {
    vi.stubGlobal('navigator', {
      clipboard: { writeText: vi.fn().mockRejectedValue(new Error('Permission denied')) },
    })
    const success = vi.spyOn(toast, 'success').mockImplementation(() => 0)
    const error = vi.spyOn(toast, 'error').mockImplementation(() => 0)
    renderDialog(model, { initialSection: 'overview' })

    fireEvent.click(screen.getByRole('button', { name: 'Copy model ID' }))

    await waitFor(() => expect(error).toHaveBeenCalledWith('Clipboard access failed'))
    expect(success).not.toHaveBeenCalled()
  })
})
