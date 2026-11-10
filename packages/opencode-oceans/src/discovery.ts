import { Effect, Schema } from 'effect'
import { record, text, type Entry } from './config'

export type Discovered = { readonly id: string; readonly entry: Entry; readonly listing?: Entry }
const richPaths = ['/model_group/info', '/v2/model/info', '/model/info', '/v1/model/info']
export const metadata = (entry: Entry, key: string) => entry[key] ?? record(entry.model_info)[key]
const identity = (entry: Entry) =>
  text(entry.model_group) ??
  text(entry.model_name) ??
  text(entry.id) ??
  text(record(entry.litellm_params).model)

function entries(body: unknown): Entry[] | undefined {
  const list = Array.isArray(body)
    ? body
    : ['data', 'models', 'result', 'items'].map((key) => record(body)[key]).find(Array.isArray)
  if (!list) return undefined
  const result = list.map(record)
  return result.every((entry) => identity(entry) !== undefined) ? result : undefined
}

export class DiscoveryError extends Schema.TaggedError<
  DiscoveryError,
  { readonly brand: unique symbol }
>()('Oceans.DiscoveryError', {
  url: Schema.String,
  reason: Schema.String,
}) {}

const fetchEntries = (url: string, key: string | undefined) =>
  Effect.gen(function* () {
    const headers: Record<string, string> = { accept: 'application/json' }
    if (key) headers.authorization = `Bearer ${key}`
    const response = yield* Effect.tryPromise((signal) =>
      fetch(url, { headers, signal, redirect: 'error' }),
    )
    // Only an absent endpoint is optional. Other failures must not replace a good inventory.
    if (response.status === 404 || response.status === 405) return undefined
    if (!response.ok) return yield* new DiscoveryError({ url, reason: `HTTP ${response.status}` })
    const result = entries(yield* Effect.tryPromise(() => response.json()))
    if (!result) return yield* new DiscoveryError({ url, reason: 'response has no model array' })
    return result
  }).pipe(
    Effect.timeout('10 seconds'),
    Effect.catchIf(
      (error) => !(error instanceof DiscoveryError),
      () => Effect.fail(new DiscoveryError({ url, reason: 'transport, timeout, or JSON failure' })),
    ),
  )

function complete(entry: Entry) {
  // Cache rates are optional, not evidence that another rich endpoint is needed.
  return (
    typeof metadata(entry, 'supports_vision') === 'boolean' &&
    ['input_cost_per_token', 'output_cost_per_token'].every((key) => {
      const value = metadata(entry, key)
      return typeof value === 'number' && Number.isFinite(value) && value >= 0
    })
  )
}

function mergeEntry(entry: Entry, previous: Entry | undefined) {
  const merged: Entry = {}
  for (const source of [record(entry.model_info), entry, record(previous?.model_info), previous]) {
    for (const [field, value] of Object.entries(source ?? {})) {
      if (value !== null && value !== undefined) merged[field] = value
    }
  }
  return merged
}

/** Rich metadata and public listing remain separate so listing hints cannot erase route facts. */
export const discover = Effect.fn('Oceans.discover')(function* (
  root: string,
  key: string | undefined,
) {
  const found = new Map<string, Entry>()
  let available = false
  for (const path of richPaths) {
    const entries = yield* fetchEntries(`${root}${path}`, key)
    if (entries !== undefined) available = true
    for (const entry of entries ?? []) {
      const id = identity(entry)
      if (!id) continue
      found.set(id, mergeEntry(entry, found.get(id)))
    }
    if (found.size && [...found.values()].every(complete)) break
  }
  const listings = new Map<string, Entry>()
  const listed = yield* fetchEntries(`${root}/v1/models`, key)
  for (const entry of listed ?? []) {
    const id = identity(entry)
    if (!id) continue
    listings.set(id, entry)
    if (!found.has(id)) found.set(id, entry)
  }
  // The authenticated list is authoritative for visibility, including model removals.
  if (listed) for (const id of found.keys()) if (!listings.has(id)) found.delete(id)
  if (!available && !listed)
    return yield* new DiscoveryError({ url: root, reason: 'no discovery endpoints available' })
  return [...found]
    .filter(
      ([, entry]) => !['embedding', 'image_generation'].includes(String(metadata(entry, 'mode'))),
    )
    .filter(([id]) => {
      const output = record(listings.get(id)?.architecture).output_modalities
      return !Array.isArray(output) || !output.includes('decisions')
    })
    .map(([id, entry]): Discovered => ({ id, entry, listing: listings.get(id) }))
    .toSorted((a, b) => a.id.localeCompare(b.id))
})
