import { Integration, Plugin, Provider } from '@opencode/plugin/effect'
import type { ProviderRecord } from '@opencode/plugin/effect/provider'
import { Effect, Schedule } from 'effect'
import { configDirectory, loadGateways, type Gateway } from './config'
import { discover, type Discovered } from './discovery'
import { catalogKey, catalogReferences, packages, toModel } from './models'

const apiKey = Effect.fn('Oceans.apiKey')(function* (ctx: Plugin.Context, gateway: Gateway) {
  const connection = yield* ctx.integration.connection.active(gateway.id)
  if (!connection) return { connection: undefined, key: undefined }
  const credential = yield* ctx.integration.connection
    .resolve(connection)
    .pipe(Effect.orElseSucceed(() => undefined))
  return { connection, key: credential?.type === 'key' ? credential.key : undefined }
})

const register = Effect.fn('Oceans.register')(function* (
  ctx: Plugin.Context,
  gateway: Gateway,
  gatewayIDs: ReadonlySet<string>,
) {
  const providerID = Provider.ID.make(gateway.id)
  const state = {
    models: [] as readonly Discovered[],
    fingerprint: '',
    connection: undefined as ProviderRecord['sourceConnection'],
  }
  yield* ctx.integration.transform((editor) => {
    editor.update(gateway.id, (integration) => {
      integration.name = gateway.name
    })
    editor.method.update({
      integrationID: gateway.id,
      method: { type: 'key', label: `${gateway.name} API key` },
    })
    if (gateway.env.length)
      editor.method.update({
        integrationID: gateway.id,
        method: { type: 'env', names: gateway.env },
      })
  })
  const sync = Effect.gen(function* () {
    const { connection, key } = yield* apiKey(ctx, gateway)
    const models = yield* discover(gateway.root, key)
    const fingerprint = JSON.stringify({ models, connection })
    if (fingerprint === state.fingerprint) return false
    state.models = models
    state.connection = connection
    state.fingerprint = fingerprint
    return true
  }).pipe(
    Effect.catchTag('Oceans.DiscoveryError', (error) =>
      Effect.logWarning('Oceans discovery failed; keeping previous inventory').pipe(
        Effect.annotateLogs({ provider: gateway.id, url: error.url, reason: error.reason }),
        Effect.as(false),
      ),
    ),
  )
  yield* sync
  yield* ctx.provider.transform((editor) => {
    const references = catalogReferences(editor.list(), gatewayIDs)
    editor.add({
      info: Object.assign(Provider.Info.empty(providerID), {
        name: gateway.name,
        activation: 'enabled' as const,
        package: packages[gateway.api],
        integrationID: Integration.ID.make(gateway.id),
        settings: { baseURL: `${gateway.root}/v1` },
      }),
      models: state.models.map(({ id, entry, listing }) =>
        toModel(
          providerID,
          id,
          entry,
          references.get(catalogKey(id)),
          gateway.modelOverrides[id],
          gateway.api,
          listing,
        ),
      ),
      sourceConnection: state.connection,
    })
  })
  yield* sync.pipe(
    Effect.flatMap((changed) => (changed ? ctx.provider.reload() : Effect.void)),
    Effect.repeat(Schedule.spaced('60 seconds')),
    Effect.delay('60 seconds'),
    Effect.forkScoped,
  )
})

export default Plugin.define({
  id: 'oceans.model-discovery',
  effect: (ctx) =>
    Effect.gen(function* () {
      const directory =
        typeof ctx.options.configDir === 'string' ? ctx.options.configDir : configDirectory()
      const gateways = yield* loadGateways(directory).pipe(
        Effect.catchTag('Oceans.ConfigFileError', (error) =>
          Effect.logWarning('Oceans gateway config unreadable').pipe(
            Effect.annotateLogs({ file: error.file, reason: error.reason }),
            Effect.as([] as readonly Gateway[]),
          ),
        ),
      )
      const gatewayIDs = new Set(gateways.map((gateway) => gateway.id))
      yield* Effect.forEach(gateways, (gateway) => register(ctx, gateway, gatewayIDs), {
        concurrency: 'unbounded',
        discard: true,
      })
    }),
})
