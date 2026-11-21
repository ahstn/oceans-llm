import { beforeEach, describe, expect, it, vi } from 'vitest'

import { loadAllModels } from './model-catalog'

const getModels = vi.hoisted(() => vi.fn())
vi.mock('@/server/admin-data.functions', () => ({ getModels }))

describe('loadAllModels', () => {
  beforeEach(() => {
    getModels.mockReset()
  })

  it('loads every page in order with at most four requests in flight', async () => {
    let inFlight = 0
    let peak = 0
    getModels.mockImplementation(async ({ data }: { data: { page: number } }) => {
      inFlight += 1
      peak = Math.max(peak, inFlight)
      await new Promise((resolve) => setTimeout(resolve, 1))
      inFlight -= 1
      return { data: { total: 1050, items: [{ id: `model-${data.page}` }] } }
    })

    const models = await loadAllModels()

    expect(getModels).toHaveBeenCalledTimes(11)
    expect(peak).toBeLessThanOrEqual(4)
    expect(models.map((model) => model.id)).toEqual(
      Array.from({ length: 11 }, (_, index) => `model-${index + 1}`),
    )
  })
})
