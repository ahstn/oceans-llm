import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const routeMock = {
  useSearch: vi.fn(),
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
}))

describe('AccountReadyPage', () => {
  beforeEach(() => {
    routeMock.useSearch.mockReturnValue({})
  })

  it('links back to the control plane after onboarding', async () => {
    const { AccountReadyPage } = await import('@/routes/account-ready')

    render(<AccountReadyPage />)

    expect(
      screen.getByText("Click below to return to the Gateway UI, if you aren't redirected."),
    ).toBeInTheDocument()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    expect(screen.queryByText('SSO onboarding complete')).not.toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Open control plane' })).toHaveAttribute(
      'href',
      '/admin',
    )
  })
})
