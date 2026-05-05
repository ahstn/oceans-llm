import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { GlobalErrorPage, getErrorMessage } from '@/components/layout/global-error-page'

describe('getErrorMessage', () => {
  it('uses Error messages when available', () => {
    expect(getErrorMessage(new Error('Unable to connect. Is the computer able to access the url?')))
      .toBe('Unable to connect. Is the computer able to access the url?')
  })

  it('falls back when the thrown value is not useful', () => {
    expect(getErrorMessage({})).toBe(
      'An unexpected error stopped the admin control plane from rendering.',
    )
  })
})

describe('GlobalErrorPage', () => {
  it('renders a clear diagnostic page for route errors', () => {
    render(
      <GlobalErrorPage
        error={new Error('Unable to connect. Is the computer able to access the url?')}
        reset={vi.fn()}
      />,
    )

    expect(screen.getByText('The admin UI could not load')).toBeInTheDocument()
    expect(
      screen.getByText('Unable to connect. Is the computer able to access the url?'),
    ).toBeInTheDocument()
    expect(screen.getByText('ADMIN_GATEWAY_ORIGIN')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument()
  })
})
