import { beforeEach, describe, expect, it, vi } from 'vitest'

import { loadAllModels } from './model-catalog'

const getModelOptions = vi.hoisted(() => vi.fn())
vi.mock('@/server/admin-data.functions', () => ({ getModelOptions }))

describe('loadAllModels', () => {
  beforeEach(() => {
    getModelOptions.mockReset()
  })

  it('loads the whole catalog in one request', async () => {
    const items = Array.from({ length: 1050 }, (_, index) => ({
      id: `model-${index}`,
    }))
    getModelOptions.mockResolvedValue({ data: { items } })

    const models = await loadAllModels()

    expect(getModelOptions).toHaveBeenCalledOnce()
    expect(models).toBe(items)
  })

  it('passes the caller signal so aborting cancels the request', async () => {
    const caller = new AbortController()
    getModelOptions.mockImplementation(
      ({ signal }: { signal: AbortSignal }) =>
        new Promise((_, reject) => {
          signal.addEventListener('abort', () => reject(new Error('aborted')), {
            once: true,
          })
        }),
    )

    const load = loadAllModels(caller.signal)
    caller.abort()

    await expect(load).rejects.toThrow('aborted')
    expect(getModelOptions.mock.calls[0][0].signal).toBe(caller.signal)
  })

  it('rejects when the request fails', async () => {
    getModelOptions.mockRejectedValue(new Error('gateway unavailable'))

    await expect(loadAllModels()).rejects.toThrow('gateway unavailable')
  })
})
