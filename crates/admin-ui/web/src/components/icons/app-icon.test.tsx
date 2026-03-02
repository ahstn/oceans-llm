import { render } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { AppIcon } from '@/components/icons/app-icon'

vi.mock('@hugeicons/react', () => ({
  HugeiconsIcon: (props: Record<string, unknown>) => (
    <div data-testid="huge-icon" data-props={JSON.stringify(props)} />
  ),
}))

describe('AppIcon', () => {
  it('uses the default icon size and stroke width', () => {
    const { container } = render(<AppIcon icon={{}} />)

    const node = container.querySelector('[data-testid="huge-icon"]')
    const props = JSON.parse(node?.getAttribute('data-props') ?? '{}')

    expect(props.size).toBe(16)
    expect(props.strokeWidth).toBe(1.2)
  })

  it('allows supported stroke variants', () => {
    const { container } = render(<AppIcon icon={{}} stroke={1.5} />)

    const node = container.querySelector('[data-testid="huge-icon"]')
    const props = JSON.parse(node?.getAttribute('data-props') ?? '{}')

    expect(props.strokeWidth).toBe(1.5)
  })
})
