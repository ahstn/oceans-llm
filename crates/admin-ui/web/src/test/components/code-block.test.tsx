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

    expect(screen.getByRole('region', { name: 'text code' })).toHaveStyle({
      maxHeight: 'calc(2 * 1.5rem + 2rem)',
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
})
