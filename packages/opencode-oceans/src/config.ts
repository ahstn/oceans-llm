import { homedir } from 'node:os'
import path from 'node:path'
import { Effect, Schema } from 'effect'

export const API = Schema.Literals(['openai-completions', 'openai-responses', 'anthropic-messages'])
export type API = typeof API.Type
const Effort = Schema.Literals(['minimal', 'low', 'medium', 'high', 'xhigh', 'max'])
export const Override = Schema.Struct({
  api: Schema.optionalKey(API),
  input: Schema.optionalKey(
    Schema.Array(Schema.Literals(['text', 'image', 'pdf', 'audio', 'video'])),
  ),
  thinking: Schema.optionalKey(
    Schema.Struct({
      mode: Schema.optionalKey(Schema.String),
      efforts: Schema.optionalKey(Schema.Array(Effort)),
      defaultLevel: Schema.optionalKey(Effort),
    }),
  ),
})
export type Override = typeof Override.Type
const GatewayConfig = Schema.Struct({
  name: Schema.optionalKey(Schema.String),
  baseUrl: Schema.String,
  apiKey: Schema.optionalKey(Schema.Union([Schema.String, Schema.Array(Schema.String)])),
  api: Schema.optionalKey(API),
  modelOverrides: Schema.optionalKey(Schema.Record(Schema.String, Override)),
})
export type Gateway = {
  readonly id: string
  readonly name: string
  readonly root: string
  readonly env: readonly string[]
  readonly api: API
  readonly modelOverrides: Readonly<Record<string, Override>>
}
export type Entry = Record<string, unknown>
export const record = (value: unknown): Entry =>
  value !== null && typeof value === 'object' && !Array.isArray(value) ? (value as Entry) : {}
export const text = (value: unknown) =>
  typeof value === 'string' && value.trim() ? value.trim() : undefined
export const configDirectory = () =>
  path.join(process.env.XDG_CONFIG_HOME || path.join(homedir(), '.config'), 'opencode')

export class ConfigFileError extends Schema.TaggedError<
  ConfigFileError,
  { readonly brand: unique symbol }
>()('Oceans.ConfigFileError', {
  file: Schema.String,
  reason: Schema.String,
}) {}
const decodeGateway = Schema.decodeUnknownEffect(GatewayConfig)

/** Only discovery.type: litellm is supported; all other OMP providers are ignored. */
export const parseGateways = Effect.fn('Oceans.parseGateways')(function* (
  file: string,
  source: string,
) {
  const document = yield* Effect.try({
    try: () => Bun.YAML.parse(source),
    catch: (cause) => new ConfigFileError({ file, reason: String(cause) }),
  })
  const gateways: Gateway[] = []
  for (const [id, value] of Object.entries(record(record(document).providers))) {
    if (record(record(value).discovery).type !== 'litellm') continue
    const config = yield* decodeGateway(value).pipe(
      Effect.flatMap((config) =>
        Effect.try({
          try: () => {
            const url = new URL(config.baseUrl)
            if (
              !['http:', 'https:'].includes(url.protocol) ||
              url.username ||
              url.password ||
              url.search ||
              url.hash
            ) {
              throw new Error(
                'baseUrl must be an HTTP(S) URL without credentials, query, or fragment',
              )
            }
            const env = typeof config.apiKey === 'string' ? [config.apiKey] : (config.apiKey ?? [])
            if (env.some((name) => !/^[A-Za-z_][A-Za-z0-9_]*$/.test(name))) {
              throw new Error(
                'apiKey must contain environment variable names, not key values or commands',
              )
            }
            return { config, env, root: url.href.replace(/\/+$/, '').replace(/\/v1$/, '') }
          },
          catch: (cause) => new ConfigFileError({ file, reason: String(cause) }),
        }),
      ),
      Effect.tapError((error) =>
        Effect.logWarning('Skipping invalid Oceans gateway').pipe(
          Effect.annotateLogs({ file, provider: id, reason: String(error) }),
        ),
      ),
      Effect.orElseSucceed(() => undefined),
    )
    if (!config) continue
    gateways.push({
      id,
      name: config.config.name ?? id,
      root: config.root,
      env: config.env,
      api: config.config.api ?? 'openai-completions',
      modelOverrides: config.config.modelOverrides ?? {},
    })
  }
  return gateways
})

/** A local file replaces the shared file, including when it is empty or invalid. */
export const loadGateways = Effect.fn('Oceans.loadGateways')(function* (directory: string) {
  for (const name of ['gateways.local.yml', 'gateways.yml']) {
    const file = path.join(directory, name)
    const exists = yield* Effect.tryPromise({
      try: () => Bun.file(file).exists(),
      catch: (cause) => new ConfigFileError({ file, reason: String(cause) }),
    })
    if (!exists) continue
    const source = yield* Effect.tryPromise({
      try: () => Bun.file(file).text(),
      catch: (cause) => new ConfigFileError({ file, reason: String(cause) }),
    })
    return yield* parseGateways(file, source)
  }
  return []
})
