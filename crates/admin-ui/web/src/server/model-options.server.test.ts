import { afterEach, describe, expect, it, vi } from 'vitest'

import { getModelOptions } from '@/server/admin-data.functions'

// The browser request that the server function serves. Aborting it stands in for a selector
// that unmounts while the catalog is still loading.
const browserRequest = vi.hoisted(() => ({ current: new Request('http://localhost:3001/') }))

// Vitest does not compile TanStack server functions, so call the handler directly. Everything
// between the handler and `fetch` (admin data wrapper, openapi-fetch, gateway fetch) is real.
vi.mock('@tanstack/react-start', () => ({
  createServerFn: () => {
    const builder = {
      validator: () => builder,
      handler: (handler: (input: { data: unknown }) => unknown) => () =>
        handler({ data: undefined }),
    }
    return builder
  },
}))

vi.mock('@tanstack/react-start/server', () => ({
  getRequest: () => browserRequest.current,
  setResponseHeader: vi.fn(),
}))

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('getModelOptions', () => {
  it('cancels the gateway request when the browser abandons the load', async () => {
    const browser = new AbortController()
    browserRequest.current = new Request('http://localhost:3001/_serverFn/options', {
      headers: { host: 'localhost:3001' },
      signal: browser.signal,
    })
    const sent: Request[] = []
    vi.stubGlobal(
      'fetch',
      vi.fn(
        (request: Request) =>
          new Promise<Response>((_resolve, reject) => {
            sent.push(request)
            request.signal.addEventListener('abort', () => reject(request.signal.reason))
          }),
      ),
    )

    const load = getModelOptions()
    await vi.waitFor(() => expect(sent).toHaveLength(1))
    expect(new URL(sent[0].url).pathname).toBe('/api/v1/admin/models/options')
    expect(sent[0].signal.aborted).toBe(false)

    browser.abort()

    await expect(load).rejects.toThrow()
    expect(sent[0].signal.aborted).toBe(true)
  })

  it('returns the gateway options when the load completes', async () => {
    browserRequest.current = new Request('http://localhost:3001/_serverFn/options', {
      headers: { host: 'localhost:3001' },
    })
    const items = [
      {
        id: 'gpt-oss-120b',
        model_id: 'uuid-1',
        resolved_model_key: 'gpt-oss-120b',
        alias_of: null,
        provider_label: 'OpenAI',
      },
    ]
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => Response.json({ data: { items }, meta: { generated_at: 'now' } })),
    )

    const response = await getModelOptions()

    expect(response.data.items).toEqual(items)
  })
})
