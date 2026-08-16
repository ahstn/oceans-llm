import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { Filters, type Filter, type FilterFieldConfig } from '@/components/reui/filters'

const fields: FilterFieldConfig<string>[] = [
  {
    key: 'user_id',
    label: 'User',
    type: 'select',
    searchable: true,
    options: [{ label: 'Alice Platform Lead', value: 'user_alice' }],
  },
]

describe('ReUI Filters', () => {
  afterEach(() => cleanup())

  it('selects an option from a searchable field submenu', async () => {
    const onChange = vi.fn<(filters: Filter<string>[]) => void>()

    render(<Filters filters={[]} fields={fields} onChange={onChange} />)

    fireEvent.pointerDown(screen.getByRole('button', { name: 'Filter' }))
    const userField = await screen.findByRole('option', { name: 'User' })
    fireEvent.click(userField)

    const alice = await screen.findByRole('option', { name: 'Alice Platform Lead' })
    fireEvent.click(alice)

    await waitFor(() =>
      expect(onChange).toHaveBeenCalledWith([
        expect.objectContaining({
          field: 'user_id',
          operator: 'is',
          values: ['user_alice'],
        }),
      ]),
    )
  })
})
