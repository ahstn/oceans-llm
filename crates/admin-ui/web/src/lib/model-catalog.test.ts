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

  it('stops claiming pages once one page fails', async () => {
    getModels.mockImplementation(async ({ data }: { data: { page: number } }) => {
      await new Promise((resolve) => setTimeout(resolve, 1))
      if (data.page === 2) throw new Error('gateway unavailable')
      return { data: { total: 2000, items: [{ id: `model-${data.page}` }] } }
    })

    await expect(loadAllModels()).rejects.toThrow('gateway unavailable')
    await new Promise((resolve) => setTimeout(resolve, 20))

    // Page 1, then the first batch of four (pages 2–5). No worker claims page 6 or later.
    expect(getModels).toHaveBeenCalledTimes(5)
    // The pages still in flight when page 2 failed are cancelled.
    const signals = getModels.mock.calls.slice(1).map(([options]) => options.signal)
    expect(signals.every((signal) => signal?.aborted)).toBe(true)
  })

  it('cancels every page in flight and claims no more when the caller aborts', async () => {
    const caller = new AbortController()
    getModels.mockImplementation(
      ({ data, signal }: { data: { page: number }; signal: AbortSignal }) => {
        if (data.page === 1) return Promise.resolve({ data: { total: 2000, items: [] } })
        // Later pages hang until aborted, as a slow request does.
        return new Promise((_, reject) => {
          signal.addEventListener('abort', () => reject(new Error('aborted')), { once: true })
        })
      },
    )

    const load = loadAllModels(caller.signal)
    await vi.waitFor(() => expect(getModels).toHaveBeenCalledTimes(5))
    caller.abort()

    await expect(load).rejects.toThrow('aborted')
    expect(getModels).toHaveBeenCalledTimes(5)
    expect(getModels.mock.calls.every(([options]) => options.signal.aborted)).toBe(true)
  })
})
