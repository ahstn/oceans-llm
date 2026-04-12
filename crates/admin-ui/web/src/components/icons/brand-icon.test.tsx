import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { BrandIcon } from '@/components/icons/brand-icon'

describe('BrandIcon', () => {
  it('renders image-backed icons like aws', () => {
    const { container } = render(<BrandIcon iconKey="aws" title="AWS" />)

    expect(container.querySelector('img')).toBeInTheDocument()
  })

  it('renders mask-backed icons like openai and openrouter', () => {
    const { rerender, container } = render(<BrandIcon iconKey="openai" title="OpenAI" />)

    expect(container.querySelector('img')).not.toBeInTheDocument()
    expect(container.firstElementChild).toHaveAttribute('title', 'OpenAI')

    rerender(<BrandIcon iconKey="openrouter" title="OpenRouter" />)

    expect(container.querySelector('img')).not.toBeInTheDocument()
    expect(container.firstElementChild).toHaveAttribute('title', 'OpenRouter')
  })
})
