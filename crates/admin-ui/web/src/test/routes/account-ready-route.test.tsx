import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AccountReadyPage } from '@/routes/account-ready'

// Hoisted with the mocks: the route module reads it as it loads.
const routeMock = vi.hoisted(() => ({
  useSearch: vi.fn(),
}))
const navigateMock = vi.fn()

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useNavigate: () => navigateMock,
}))

describe('AccountReadyPage', () => {
  beforeEach(() => {
    routeMock.useSearch.mockReturnValue({})
    navigateMock.mockReset()
  })

  afterEach(() => {
    cleanup()
    vi.useRealTimers()
  })

  it('links back to the control plane after onboarding', () => {
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
    expect(navigateMock).not.toHaveBeenCalled()
  })

  it('redirects SSO onboarding to the gateway UI after showing the success state', async () => {
    vi.useFakeTimers()
    routeMock.useSearch.mockReturnValue({ mode: 'oauth' })

    render(<AccountReadyPage />)

    expect(screen.getByRole('link', { name: 'Open control plane' })).toBeInTheDocument()
    expect(navigateMock).not.toHaveBeenCalled()

    await vi.runAllTimersAsync()

    expect(navigateMock).toHaveBeenCalledWith({ to: '/' })
  })
})
