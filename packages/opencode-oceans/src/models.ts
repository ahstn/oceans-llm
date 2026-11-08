import { Model, Provider } from '@opencode/plugin/effect'
import type { ProviderRecord } from '@opencode/plugin/effect/provider'
import { record, text, type API, type Entry, type Override } from './config'
import { metadata } from './discovery'

export const packages = {
  'openai-completions': '@opencode/ai/providers/openai-compatible',
  'openai-responses': '@opencode/ai/providers/openai-compatible/responses',
  'anthropic-messages': '@opencode/ai/providers/anthropic-compatible',
} as const satisfies Record<API, string>
const effortNames = new Set(['minimal', 'low', 'medium', 'high', 'xhigh', 'max'])
const vendors = [
  'anthropic',
  'openai',
  'google',
  'xai',
  'deepseek',
  'mistral',
  'moonshotai',
  'zai',
  'alibaba',
  'minimax',
]
const strings = (value: unknown) =>
  Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string')
    : undefined
const tokens = (value: unknown) =>
  typeof value === 'number' && Number.isSafeInteger(value) && value > 0 ? value : undefined
const rate = (value: unknown) => {
  const number = typeof value === 'string' && value.trim() ? Number(value) : value
  return typeof number === 'number' && Number.isFinite(number) && number >= 0 ? number : undefined
}
export const catalogKey = (id: string) => id.replaceAll('.', '-')

function listedApi(listing: Entry): API | undefined {
  const hints = record(listing.client_hints)
  const preferred = text(hints.preferred_api_format)
  const formats = strings(hints.api_formats)
  if (preferred && (!formats || formats.includes(preferred))) {
    if (preferred === 'anthropic-messages') return 'anthropic-messages'
    if (preferred === 'openai-responses') return 'openai-responses'
    if (preferred === 'openai-chat-completions') return 'openai-completions'
  }
  const endpoints = strings(listing.supported_endpoint_types)
  if (endpoints?.includes('anthropic') && (!formats || formats.includes('anthropic-messages')))
    return 'anthropic-messages'
  if (endpoints?.[0] === 'openai-response' && (!formats || formats.includes('openai-responses')))
    return 'openai-responses'
  return undefined
}

/** Rich route pools take precedence over primary-route preferences on mixed gateways. */
export function apiFor(
  entry: Entry,
  id: string,
  fallback: API = 'openai-completions',
  listing: Entry = entry,
): API {
  const routes = strings(entry.providers)
    ?.map((route) => route.trim().toLowerCase())
    .filter(Boolean)
  if (routes?.length && routes.every((route) => route === 'openai')) return 'openai-responses'
  if (routes && new Set(routes).size > 1) return 'openai-completions'
  const params = record(entry.litellm_params)
  const provider = text(params.custom_llm_provider)?.toLowerCase()
  const base = text(params.model) ?? text(metadata(entry, 'base_model'))
  if (!routes?.length && (provider === 'openai' || (!provider && base?.startsWith('openai/'))))
    return 'openai-responses'
  const advertised = listedApi(listing)
  if (advertised) return advertised
  if (routes?.length || provider || base?.includes('/')) {
    // Vendor identity alone is not a guarantee of native Anthropic transport (e.g. Converse).
    return 'openai-completions'
  }
  return /^(?:openai\/)?(?:gpt-|chatgpt-|o\d(?:-|$)|codex-)/i.test(id)
    ? 'openai-responses'
    : fallback
}

function costs(entry: Entry, listing: Entry, reference?: Model.Info): Model.Info['cost'] {
  const pricing = record(listing.pricing)
  const input = rate(metadata(entry, 'input_cost_per_token')) ?? rate(pricing.prompt)
  const output = rate(metadata(entry, 'output_cost_per_token')) ?? rate(pricing.completion)
  const read =
    rate(metadata(entry, 'cache_read_input_token_cost')) ?? rate(pricing.input_cache_read)
  const write =
    rate(metadata(entry, 'cache_creation_input_token_cost')) ?? rate(pricing.input_cache_write)
  if ([input, output, read, write].every((value) => value === undefined))
    return reference?.cost ?? []
  const usd = (value: number) => Model.Cost.fields.input.make(value)
  const first = reference?.cost[0]
  const inputRate = input === undefined ? (first?.input ?? 0) : input * 1_000_000
  const outputRate = output === undefined ? (first?.output ?? 0) : output * 1_000_000
  return [
    {
      input: usd(inputRate),
      output: usd(outputRate),
      cache: {
        // Missing cache rates do not make cached tokens free. Prefer catalog, then input rate.
        read: usd(read === undefined ? (first?.cache.read ?? inputRate) : read * 1_000_000),
        write: usd(write === undefined ? (first?.cache.write ?? inputRate) : write * 1_000_000),
      },
    },
  ]
}

function capabilities(
  entry: Entry,
  listing: Entry,
  api: API,
  reference?: Model.Info,
  override?: Override,
): Model.Info['capabilities'] {
  const declared = metadata(entry, 'supports_function_calling')
  const params = strings(metadata(entry, 'supported_openai_params'))
  const tools = record(listing.capabilities).tools
  const toolSupport = typeof tools === 'boolean' ? tools : record(tools).supported
  const vision = metadata(entry, 'supports_vision')
  const architecture = record(listing.architecture)
  let input = strings(architecture.input_modalities) ?? reference?.capabilities.input ?? ['text']
  if (vision === false) input = input.filter((modality) => modality !== 'image')
  if (vision === true && !input.includes('image')) input = [...input, 'image']
  if (api === 'anthropic-messages') {
    for (const [modality, field] of [
      ['image', 'image_input'],
      ['pdf', 'pdf_input'],
    ] as const) {
      const support = record(record(listing.capabilities)[field]).supported
      if (support === false) input = input.filter((value) => value !== modality)
      if (support === true && !input.includes(modality)) input = [...input, modality]
    }
  }
  return {
    tools:
      typeof declared === 'boolean'
        ? declared
        : params
          ? params.some((param) =>
              ['tools', 'tool_choice', 'functions', 'function_call'].includes(param),
            )
          : typeof toolSupport === 'boolean'
            ? toolSupport
            : (reference?.capabilities.tools ?? true),
    input: override?.input ?? input,
    output: strings(architecture.output_modalities) ?? reference?.capabilities.output ?? ['text'],
    temperature: params ? params.includes('temperature') : reference?.capabilities.temperature,
  }
}

type EffortVariant = { id: string; effort: string }
function hintedVariants(listing: Entry): EffortVariant[] | undefined {
  const hints = record(record(record(listing.client_hints).harnesses).opencode)
  if (hints.variants === undefined) return undefined
  const entries = Array.isArray(hints.variants)
    ? hints.variants.map((variant) => [text(record(variant).id), record(variant).settings] as const)
    : Object.entries(record(hints.variants))
  return entries.flatMap(([id, settings]) => {
    const effort = text(record(settings).effort) ?? text(record(settings).reasoningEffort) ?? id
    return id && effort && effortNames.has(id) && effortNames.has(effort) ? [{ id, effort }] : []
  })
}

function effortsFor(
  entry: Entry,
  listing: Entry,
  api: API,
  reference?: Model.Info,
  override?: Override,
): EffortVariant[] {
  if (override?.thinking?.efforts)
    return override.thinking.efforts.map((id) => ({ id, effort: id }))
  if (metadata(entry, 'supports_reasoning') === false) return []
  const native = record(record(listing.capabilities).effort)
  const hinted = hintedVariants(listing)
  if (api === 'anthropic-messages' && typeof native.supported === 'boolean') {
    if (!native.supported) return []
    const declared = [...effortNames].filter(
      (level) => typeof record(native[level]).supported === 'boolean',
    )
    if (declared.length) {
      const levels = declared.filter((level) => record(native[level]).supported === true)
      // Explicit ladders are authoritative; missing per-level metadata is not a denial.
      return levels.map((id) => ({
        id,
        effort:
          hinted?.find((variant) => variant.id === id && levels.includes(variant.effort))?.effort ??
          id,
      }))
    }
    if (hinted) return hinted
  }
  const params = strings(metadata(entry, 'supported_openai_params'))
  if (params && !params.includes('reasoning_effort')) return []
  const advertised = strings(metadata(entry, 'supported_reasoning_efforts'))
  if (advertised)
    return advertised.filter((id) => effortNames.has(id)).map((id) => ({ id, effort: id }))
  // Hints already encode policy and ceilings. Do not grow them from the vendor catalog.
  if (hinted) return hinted
  return (
    reference?.variants
      .filter((variant) => effortNames.has(variant.id))
      .map((variant) => ({ id: variant.id, effort: variant.id })) ?? []
  )
}

function effortSettings(api: API, effort: string, adaptive: boolean): Model.Settings {
  if (api === 'anthropic-messages')
    return adaptive ? { thinking: { type: 'adaptive' }, effort } : { effort }
  if (api === 'openai-responses') return { reasoningEffort: effort, reasoningSummary: 'auto' }
  return { reasoningEffort: effort }
}

function reasoning(
  api: API,
  entry: Entry,
  listing: Entry,
  output: number,
  reference?: Model.Info,
  override?: Override,
) {
  const thinking = record(record(listing.capabilities).thinking)
  const adaptive =
    override?.thinking?.mode === 'anthropic-adaptive' ||
    (metadata(entry, 'supports_reasoning') !== false &&
      thinking.supported !== false &&
      record(record(thinking.types).adaptive).supported === true)
  const efforts = effortsFor(entry, listing, api, reference, override)
  const manual =
    api === 'anthropic-messages' &&
    !adaptive &&
    metadata(entry, 'supports_reasoning') !== false &&
    thinking.supported === true &&
    record(record(thinking.types).enabled).supported === true
  const maximumBudget = Math.min(32_000, output) - 1
  const variants =
    manual && maximumBudget >= 1024 && !efforts.length
      ? [
          {
            id: Model.VariantID.make('high'),
            settings: {
              thinking: {
                type: 'enabled',
                budgetTokens: Math.max(1024, Math.floor((maximumBudget + 1) / 2)),
              },
            },
          },
          {
            id: Model.VariantID.make('max'),
            settings: { thinking: { type: 'enabled', budgetTokens: maximumBudget } },
          },
        ]
      : efforts.map(({ id, effort }) => ({
          id: Model.VariantID.make(id),
          settings: {
            ...effortSettings(api, effort, adaptive),
            ...(manual && output > 1024
              ? { thinking: { type: 'enabled', budgetTokens: Math.min(16_000, output - 1) } }
              : {}),
          },
        }))
  const defaultLevel = override?.thinking?.defaultLevel
  const defaultVariant = defaultLevel && variants.find((variant) => variant.id === defaultLevel)
  return {
    settings: defaultVariant
      ? defaultVariant.settings
      : api === 'anthropic-messages' && adaptive
        ? { thinking: { type: 'adaptive' } }
        : undefined,
    variants,
  }
}

export function toModel(
  providerID: Provider.ID,
  id: string,
  entry: Entry,
  reference?: Model.Info,
  override?: Override,
  fallback?: API,
  listing: Entry = entry,
): Model.Info {
  const api = override?.api ?? apiFor(entry, id, fallback, listing)
  const context =
    tokens(listing.context_length) ??
    tokens(metadata(entry, 'max_input_tokens')) ??
    tokens(listing.max_input_tokens) ??
    reference?.limit.context ??
    128_000
  const input = Math.min(
    context,
    tokens(metadata(entry, 'max_input_tokens')) ??
      tokens(listing.max_input_tokens) ??
      reference?.limit.input ??
      context,
  )
  const output =
    tokens(metadata(entry, 'max_output_tokens')) ??
    tokens(listing.max_output_tokens) ??
    tokens(listing.max_tokens) ??
    reference?.limit.output ??
    Math.min(context, 32_768)
  return {
    ...Model.Info.default(providerID, Model.ID.make(id)),
    name: text(listing.display_name) ?? text(listing.name) ?? reference?.name ?? id,
    family: reference?.family,
    package: packages[api],
    capabilities: capabilities(entry, listing, api, reference, override),
    limit: { context, input, output },
    cost: costs(entry, listing, reference),
    ...reasoning(api, entry, listing, output, reference, override),
  }
}

/** First-party references win; one gateway must never seed another gateway's fallbacks. */
export function catalogReferences(
  records: readonly ProviderRecord[],
  gatewayIDs: ReadonlySet<string>,
): ReadonlyMap<string, Model.Info> {
  const rank = (id: string) => (vendors.indexOf(id) < 0 ? vendors.length : vendors.indexOf(id))
  const sources = records
    .filter((source) => !gatewayIDs.has(source.provider.id) && vendors.includes(source.provider.id))
    .toSorted((a, b) => rank(a.provider.id) - rank(b.provider.id))
  const references = new Map<string, Model.Info>()
  for (const source of sources)
    for (const model of source.models.values()) {
      const key = catalogKey(model.id)
      if (!references.has(key)) references.set(key, model)
    }
  return references
}
