import { act, cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { PageHeader } from '@/components/layout/page-header'

describe('PageHeader leading element', () => {
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('keeps the measured leading element capped and the text shrinkable', () => {
    let report: ((blockSize: number) => void) | undefined
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(callback: ResizeObserverCallback) {
          report = (blockSize) =>
            callback(
              [{ borderBoxSize: [{ blockSize, inlineSize: 0 }] } as unknown as ResizeObserverEntry],
              this as unknown as ResizeObserver,
            )
        }
        observe() {}
        disconnect() {}
      },
    )

    render(
      <PageHeader
        section="Profile"
        title="Welcome back"
        description="A long description that wraps on narrow screens."
        leading={<span>AV</span>}
      />,
    )
    // A narrow layout wraps the text, so the measured height runs away.
    act(() => report?.(400))

    const leading = screen.getByTestId('page-header-leading')
    expect(leading.style.height).toBe('400px')
    // The caps, not the measurement, bound the rendered size, which breaks the feedback loop.
    for (const cap of ['max-h-24', 'max-w-24', 'max-sm:max-h-14', 'max-sm:max-w-14', 'shrink-0']) {
      expect(leading).toHaveClass(cap)
    }
    expect(screen.getByTestId('page-header-text')).toHaveClass('min-w-0', 'flex-1')
  })

  it('renders without a leading element by default', () => {
    render(<PageHeader section="Keys" title="API keys" description="Manage keys." />)
    expect(screen.queryByTestId('page-header-leading')).toBeNull()
  })
})
