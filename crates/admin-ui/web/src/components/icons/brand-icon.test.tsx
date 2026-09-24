import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { BrandIcon } from '@/components/icons/brand-icon'

describe('BrandIcon', () => {
  it('renders image-backed icons like aws', () => {
    const { container } = render(<BrandIcon iconKey="aws" title="AWS" />)

    expect(container.querySelector('img')).toBeInTheDocument()
  })

  it('renders image-backed model icons like deepseek and qwen', () => {
    const { container, rerender } = render(<BrandIcon iconKey="deepseek" title="DeepSeek" />)

    expect(container.querySelector('img')).toBeInTheDocument()

    rerender(<BrandIcon iconKey="qwen" title="Qwen" />)
    expect(container.querySelector('img')).toBeInTheDocument()
  })

  it('renders inline svg icons like openai', () => {
    const { container } = render(<BrandIcon iconKey="openai" title="OpenAI" />)

    expect(container.querySelector('svg')).toBeInTheDocument()
    expect(container.firstElementChild).toHaveAttribute('title', 'OpenAI')
  })

  it('renders inline svg icons like openrouter', () => {
    const { container } = render(<BrandIcon iconKey="openrouter" title="OpenRouter" />)

    expect(container.querySelector('svg')).toBeInTheDocument()
    expect(container.querySelector('img')).not.toBeInTheDocument()
    expect(container.firstElementChild).toHaveAttribute('title', 'OpenRouter')
  })

  it('renders inline svg icons like typesafe', () => {
    const { container } = render(<BrandIcon iconKey="typesafe" title="TypeSafe" />)

    expect(container.querySelector('svg')).toBeInTheDocument()
    expect(container.querySelector('img')).not.toBeInTheDocument()
    expect(container.firstElementChild).toHaveAttribute('title', 'TypeSafe')
  })
})
