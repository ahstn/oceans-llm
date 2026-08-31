import type { ReactNode } from 'react'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

vi.mock('recharts', async (importOriginal) => {
  const recharts = await importOriginal<typeof import('recharts')>()
  return {
    ...recharts,
    ResponsiveContainer: ({ children }: { children: ReactNode }) => children,
  }
})

import { ChartContainer, ChartLegendContent, ChartTooltipContent } from '@/components/ui/chart'

const tooltipItem = {
  dataKey: 'requests',
  name: 'Requests',
  value: 10,
  color: '#000',
  type: 'line' as const,
  payload: {},
  graphicalItemId: 'requests-series',
}

function Tooltip({ values }: { values: number[] }) {
  return (
    <ChartContainer config={{ requests: { label: 'Requests' } }}>
      <ChartTooltipContent
        active
        payload={values.map((value) => ({ ...tooltipItem, value }))}
        formatter={(itemValue, _name, _item, index) => (
          <span data-testid={`tooltip-value-${index}`}>{String(itemValue)}</span>
        )}
      />
    </ChartContainer>
  )
}

describe('chart content', () => {
  it('keeps duplicate series rows unique and stable when values change', () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    const { rerender } = render(<Tooltip values={[10, 20]} />)
    const firstRow = screen.getByTestId('tooltip-value-0').parentElement
    const secondRow = screen.getByTestId('tooltip-value-1').parentElement

    rerender(<Tooltip values={[30, 40]} />)

    expect(screen.getByTestId('tooltip-value-0').parentElement).toBe(firstRow)
    expect(screen.getByTestId('tooltip-value-1').parentElement).toBe(secondRow)
    expect(consoleError.mock.calls.some(([message]) => String(message).includes('same key'))).toBe(
      false,
    )
    consoleError.mockRestore()
  })

  it('assigns unique keys to duplicate legend entries', () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    const duplicateItem = {
      dataKey: 'requests',
      value: 'Requests',
      color: '#000',
      type: 'line' as const,
      payload: {},
    }

    render(
      <ChartContainer config={{ requests: { label: 'Requests' } }}>
        <ChartLegendContent payload={[duplicateItem, duplicateItem]} />
      </ChartContainer>,
    )

    expect(consoleError.mock.calls.some(([message]) => String(message).includes('same key'))).toBe(
      false,
    )
    consoleError.mockRestore()
  })
})
