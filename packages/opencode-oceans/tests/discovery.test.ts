import { expect, test } from 'bun:test'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { Effect } from 'effect'
import { loadGateways, parseGateways } from '../src/config'
import { discover } from '../src/discovery'

test('a local gateway file replaces shared config, without executing OMP key commands', async () => {
  const dir = await mkdtemp(path.join(tmpdir(), 'oceans-config-'))
  try {
    await Bun.write(
      path.join(dir, 'gateways.yml'),
      'providers:\n  shared:\n    baseUrl: https://example.com/v1\n    discovery: {type: litellm}\n',
    )
    await Bun.write(path.join(dir, 'gateways.local.yml'), 'providers: {}\n')
    expect(await Effect.runPromise(loadGateways(dir))).toEqual([])
    expect(
      await Effect.runPromise(
        parseGateways(
          'config',
          "providers:\n  unsafe:\n    baseUrl: https://example.com\n    apiKey: '!fnox get KEY'\n    discovery: {type: litellm}\n",
        ),
      ),
    ).toEqual([])
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
})

test('discovery merges rich gaps, applies list visibility, and does not confuse zero cost with missing data', async () => {
  const requests: string[] = []
  const server = Bun.serve({
    port: 0,
    fetch(request) {
      expect(request.headers.get('authorization')).toBe('Bearer synthetic-key')
      const pathname = new URL(request.url).pathname
      requests.push(pathname)
      if (pathname === '/model_group/info')
        return Response.json({
          data: [
            {
              model_group: 'active',
              supports_vision: false,
              input_cost_per_token: 0,
              output_cost_per_token: 0,
            },
            {
              model_group: 'removed',
              supports_vision: false,
              input_cost_per_token: 0,
              output_cost_per_token: 0,
            },
          ],
        })
      if (pathname === '/v1/models')
        return Response.json({ data: [{ id: 'active', display_name: 'Visible' }] })
      return new Response('missing', { status: 404 })
    },
  })
  try {
    const models = await Effect.runPromise(discover(server.url.origin, 'synthetic-key'))
    expect(models).toEqual([
      {
        id: 'active',
        entry: {
          model_group: 'active',
          supports_vision: false,
          input_cost_per_token: 0,
          output_cost_per_token: 0,
        },
        listing: { id: 'active', display_name: 'Visible' },
      },
    ])
    expect(requests).toEqual(['/model_group/info', '/v1/models'])
  } finally {
    await server.stop(true)
  }
})

test('authorization and malformed metadata errors fail discovery rather than accepting a partial list', async () => {
  let malformed = false
  const server = Bun.serve({
    port: 0,
    fetch() {
      return malformed ? Response.json({ unexpected: [] }) : new Response('denied', { status: 403 })
    },
  })
  try {
    const denied = await Effect.runPromise(Effect.result(discover(server.url.origin, undefined)))
    expect(denied._tag).toBe('Failure')
    malformed = true
    const invalid = await Effect.runPromise(Effect.result(discover(server.url.origin, undefined)))
    expect(invalid._tag).toBe('Failure')
  } finally {
    await server.stop(true)
  }
})

test('an authoritative empty listing removes the inventory', async () => {
  const server = Bun.serve({
    port: 0,
    fetch(request) {
      return new URL(request.url).pathname === '/model_group/info'
        ? Response.json({
            data: [
              {
                model_group: 'old',
                supports_vision: true,
                input_cost_per_token: 1,
                output_cost_per_token: 1,
              },
            ],
          })
        : Response.json({ data: [] })
    },
  })
  try {
    expect(await Effect.runPromise(discover(server.url.origin, undefined))).toEqual([])
  } finally {
    await server.stop(true)
  }
})
