import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  CodeBlock,
  CodeBlockCopyButton,
  CodeBlockHeader,
  CodeBlockTitle,
} from '@/components/reui/code-block/code-block'

describe('CodeBlock', () => {
  afterEach(cleanup)

  it('bounds its viewport and reports an unavailable clipboard', async () => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: undefined,
    })
    const onCopyError = vi.fn()

    render(
      <CodeBlock code={'first\nsecond\nthird'} language="text" showLineNumbers maxLines={2}>
        <CodeBlockHeader>
          <CodeBlockTitle>settings.txt</CodeBlockTitle>
          <CodeBlockCopyButton onCopyError={onCopyError} />
        </CodeBlockHeader>
      </CodeBlock>,
    )

    const viewport = screen.getByRole('region', { name: 'text code' })
    expect(viewport.closest('[data-slot="code-block"]')).toHaveClass(
      'min-w-0',
      'max-w-full',
      'overflow-hidden',
    )
    expect(viewport).toHaveClass('min-w-0', 'max-w-full', 'overflow-auto')
    expect(viewport).toHaveStyle({
      // Computed calc(2 * 1.5rem + 2rem) at a 16px root.
      maxHeight: '80px',
    })
    fireEvent.click(screen.getByRole('button', { name: 'Copy code' }))

    await waitFor(() => expect(onCopyError).toHaveBeenCalledOnce())
    expect(screen.getByRole('button', { name: 'Copy failed' })).toHaveAttribute(
      'data-copy-state',
      'failed',
    )
    expect(screen.getByRole('button', { name: 'Copy failed' })).toContainElement(
      document.querySelector('[data-copy-error-icon]'),
    )
  })

  it('scrolls long lines by default and soft-wraps them with wrapLines', () => {
    const { rerender } = render(<CodeBlock code="a long line" language="text" />)
    const pre = () => screen.getByRole('region', { name: 'text code' }).querySelector('pre')
    const line = () => document.querySelector('[data-slot="code-block-line"] > span')

    expect(pre()).toHaveClass('min-w-max')
    expect(line()).toHaveClass('whitespace-pre')

    rerender(<CodeBlock code="a long line" language="text" wrapLines />)
    expect(pre()).not.toHaveClass('min-w-max')
    expect(line()).toHaveClass('whitespace-pre-wrap', 'break-words')
  })
})
