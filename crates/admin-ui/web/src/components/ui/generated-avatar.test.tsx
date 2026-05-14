import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

vi.mock('boring-avatars', () => ({
  default: ({ name, variant, colors, ...props }: Record<string, unknown>) => (
    <svg
      data-testid={String(props['data-testid'])}
      data-name={String(name)}
      data-variant={variant ? String(variant) : ''}
      data-colors={Array.isArray(colors) ? colors.join(',') : ''}
      aria-label={String(props['aria-label'])}
    />
  ),
}))

import { GeneratedAvatar, OCEANS_AVATAR_COLORS } from '@/components/ui/generated-avatar'

describe('GeneratedAvatar', () => {
  it('renders team avatars without an explicit variant', () => {
    render(<GeneratedAvatar kind="team" name="Core Platform" />)

    const avatar = screen.getByLabelText('Team avatar for Core Platform')
    expect(avatar).toHaveAttribute('data-testid', 'team-generated-avatar')
    expect(avatar).toHaveAttribute('data-name', 'Core Platform')
    expect(avatar).toHaveAttribute('data-variant', '')
    expect(avatar).toHaveAttribute('data-colors', OCEANS_AVATAR_COLORS.join(','))
  })

  it('renders user avatars with the beam variant', () => {
    render(<GeneratedAvatar kind="user" name="Alice Paul" />)

    const avatar = screen.getByLabelText('User avatar for Alice Paul')
    expect(avatar).toHaveAttribute('data-testid', 'user-generated-avatar')
    expect(avatar).toHaveAttribute('data-name', 'Alice Paul')
    expect(avatar).toHaveAttribute('data-variant', 'beam')
  })
})
