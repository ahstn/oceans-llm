import { describe, expect, test } from 'bun:test'
import assert from 'node:assert/strict'
import { Model, Provider } from '@opencode/plugin/effect'
import { toModel, apiFor } from '../src/models'

const provider = Provider.ID.make('oceans')
const hints = (format: string, variants?: unknown) => ({
  client_hints: {
    preferred_api_format: format,
    api_formats: [format],
    harnesses: { opencode: { variants } },
  },
})

describe('transport selection', () => {
  test('native Anthropic hints beat fallback but do not change mixed route pools', () => {
    const listing = hints('anthropic-messages')
    expect(apiFor({ providers: ['anthropic'] }, 'alias', 'openai-completions', listing)).toBe(
      'anthropic-messages',
    )
    expect(
      apiFor({ providers: ['anthropic', 'openai'] }, 'alias', 'anthropic-messages', listing),
    ).toBe('openai-completions')
    expect(apiFor({ providers: ['openai'] }, 'alias', 'anthropic-messages', listing)).toBe(
      'openai-responses',
    )
    expect(apiFor({ providers: ['bedrock'] }, 'claude-opus-5.5')).toBe('openai-completions')
  })
  test('explicit model override wins over route evidence', () => {
    expect(
      toModel(provider, 'alias', { providers: ['openai'] }, undefined, {
        api: 'anthropic-messages',
      }).package,
    ).toBe('@opencode/ai/providers/anthropic-compatible')
  })
})

test('partial native effort metadata preserves hinted presets but explicit denials do not', () => {
  const listing = {
    ...hints('anthropic-messages', [
      { id: 'low', settings: { effort: 'low' } },
      { id: 'high', settings: { effort: 'high' } },
    ]),
    capabilities: {
      thinking: { supported: true, types: { adaptive: { supported: true } } },
      effort: { supported: true },
    },
  }
  const model = toModel(
    provider,
    'claude',
    { supported_openai_params: ['tools'] },
    undefined,
    undefined,
    undefined,
    listing,
  )
  assert.deepEqual(model.variants, [
    { id: 'low', settings: { thinking: { type: 'adaptive' }, effort: 'low' } },
    { id: 'high', settings: { thinking: { type: 'adaptive' }, effort: 'high' } },
  ])
  expect(
    toModel(provider, 'claude', {}, undefined, undefined, undefined, {
      ...listing,
      capabilities: {
        effort: { supported: true, low: { supported: false }, high: { supported: false } },
      },
    }).variants,
  ).toEqual([])
})

describe('reasoning policy', () => {
  test('native effort ladder and preset remapping respect ceiling', () => {
    const model = toModel(
      provider,
      'claude',
      { supports_reasoning: true },
      undefined,
      undefined,
      undefined,
      {
        ...hints('anthropic-messages', [{ id: 'max', settings: { effort: 'xhigh' } }]),
        capabilities: {
          thinking: { supported: true, types: { adaptive: { supported: true } } },
          effort: {
            supported: true,
            low: { supported: true },
            xhigh: { supported: true },
            max: { supported: true },
          },
        },
      },
    )
    assert.deepEqual(
      model.variants.map((v) => v.id),
      ['low', 'xhigh', 'max'],
    )
    expect(model.variants.find((v) => v.id === 'max')?.settings).toEqual({
      thinking: { type: 'adaptive' },
      effort: 'xhigh',
    })
    expect(model.settings).toEqual({ thinking: { type: 'adaptive' } })
  })
  test('OpenAI hints prevent catalog variants above the ceiling', () => {
    const reference = {
      ...Model.Info.default(provider, Model.ID.make('gpt')),
      variants: [{ id: Model.VariantID.make('max'), settings: { reasoningEffort: 'max' } }],
    }
    const model = toModel(
      provider,
      'gpt',
      {
        providers: ['openai'],
        supports_reasoning: true,
        supported_openai_params: ['reasoning_effort'],
      },
      reference,
      undefined,
      undefined,
      hints('openai-responses', [{ id: 'low', settings: { reasoningEffort: 'low' } }]),
    )
    assert.deepEqual(model.variants, [
      { id: 'low', settings: { reasoningEffort: 'low', reasoningSummary: 'auto' } },
    ])
  })
  test('explicit false reasoning suppresses catalog fallback and adaptive defaults', () => {
    const model = toModel(
      provider,
      'alias',
      { supports_reasoning: false },
      undefined,
      undefined,
      undefined,
      {
        ...hints('anthropic-messages'),
        capabilities: { thinking: { supported: true, types: { adaptive: { supported: true } } } },
      },
    )
    expect(model.variants).toEqual([])
    expect(model.settings).toBeUndefined()
  })
  test('manual Messages budgets fit the output limit and never send adaptive thinking', () => {
    const model = toModel(
      provider,
      'legacy',
      { max_output_tokens: 4096, supports_reasoning: true },
      undefined,
      undefined,
      undefined,
      {
        ...hints('anthropic-messages'),
        capabilities: {
          effort: { supported: false },
          thinking: {
            supported: true,
            types: { enabled: { supported: true }, adaptive: { supported: false } },
          },
        },
      },
    )
    assert.deepEqual(model.variants, [
      { id: 'high', settings: { thinking: { type: 'enabled', budgetTokens: 2048 } } },
      { id: 'max', settings: { thinking: { type: 'enabled', budgetTokens: 4095 } } },
    ])
  })
  test('default effort is only applied when it is in the allowed ladder', () => {
    const model = toModel(
      provider,
      'alias',
      { providers: ['openai'], supported_reasoning_efforts: ['low', 'high'] },
      undefined,
      { thinking: { defaultLevel: 'max' } },
    )
    expect(model.settings).toBeUndefined()
  })
})

describe('modalities and limits', () => {
  test('catalog input ceilings survive missing metadata and tighter gateway contexts', () => {
    const reference = {
      ...Model.Info.default(provider, Model.ID.make('alias')),
      limit: { context: 128_000, input: 100_000, output: 4096 },
    }
    expect(toModel(provider, 'alias', {}, reference).limit).toEqual(reference.limit)
    expect(
      toModel(provider, 'alias', {}, reference, undefined, undefined, {
        context_length: 80_000,
      }).limit,
    ).toEqual({ context: 80_000, input: 80_000, output: 4096 })
  })

  test('independent image and PDF declarations are honored', () => {
    const model = toModel(
      provider,
      'alias',
      { supports_vision: false, max_input_tokens: 100_000 },
      undefined,
      undefined,
      undefined,
      {
        ...hints('anthropic-messages'),
        context_length: 128_000,
        architecture: { input_modalities: ['text', 'image', 'pdf'] },
        capabilities: { image_input: { supported: false }, pdf_input: { supported: true } },
      },
    )
    expect(model.capabilities.input).toEqual(['text', 'pdf'])
    expect(model.limit).toEqual({ context: 128_000, input: 100_000, output: 32_768 })
  })
  test('vision enables images without promising PDF and override is authoritative', () => {
    expect(toModel(provider, 'alias', { supports_vision: true }).capabilities.input).toEqual([
      'text',
      'image',
    ])
    expect(
      toModel(provider, 'alias', { supports_vision: false }, undefined, {
        input: ['text', 'image'],
      }).capabilities.input,
    ).toEqual(['text', 'image'])
  })
})

describe('cost units', () => {
  test('per-token rates become USD per million and explicit free cache remains free', () => {
    const model = toModel(provider, 'alias', {
      input_cost_per_token: 0.000003,
      output_cost_per_token: 0.000015,
      cache_read_input_token_cost: 0,
      cache_creation_input_token_cost: 0.00000375,
    })
    assert.deepEqual(model.cost, [{ input: 3, output: 15, cache: { read: 0, write: 3.75 } }])
  })
  test('partial rates use catalog instead of turning the missing direction free', () => {
    const reference = {
      ...Model.Info.default(provider, Model.ID.make('alias')),
      cost: [
        {
          input: Model.Cost.fields.input.make(3),
          output: Model.Cost.fields.output.make(15),
          cache: {
            read: Model.Cost.fields.input.make(0.3),
            write: Model.Cost.fields.input.make(3.75),
          },
        },
      ],
    }
    assert.deepEqual(
      toModel(provider, 'alias', { output_cost_per_token: 0.00001 }, reference).cost,
      [{ input: 3, output: 10, cache: { read: 0.3, write: 3.75 } }],
    )
    assert.deepEqual(
      toModel(provider, 'alias', { cache_read_input_token_cost: 0 }, reference).cost,
      [{ input: 3, output: 15, cache: { read: 0, write: 3.75 } }],
    )
    assert.deepEqual(
      toModel(provider, 'alias', {}, reference, undefined, undefined, {
        pricing: { input_cache_write: 0.000002 },
      }).cost,
      [{ input: 3, output: 15, cache: { read: 0.3, write: 2 } }],
    )
  })
})

describe('service tiers', () => {
  test('optional tiers become variants spelled for the package', () => {
    const openai = toModel(
      provider,
      'gpt',
      { supported_openai_params: ['service_tier'] },
      undefined,
      undefined,
      undefined,
      {
        ...hints('openai-chat-completions'),
        service_tiers: ['default', 'flex', 'priority', 'ultrafast'],
      },
    )
    assert.deepEqual(openai.variants, [
      { id: 'flex', settings: { serviceTier: 'flex' } },
      { id: 'fast', settings: { serviceTier: 'priority' } },
      { id: 'ultrafast', settings: { serviceTier: 'ultrafast' } },
    ])

    // Anthropic sends Fast as `speed`, after the effort presets and with the default thinking.
    const claude = toModel(provider, 'claude', {}, undefined, undefined, undefined, {
      ...hints('anthropic-messages', [{ id: 'high', settings: { effort: 'high' } }]),
      capabilities: {
        thinking: { supported: true, types: { adaptive: { supported: true } } },
        effort: { supported: true },
      },
      service_tiers: ['default', 'priority'],
    })
    assert.deepEqual(claude.variants, [
      { id: 'high', settings: { thinking: { type: 'adaptive' }, effort: 'high' } },
      { id: 'fast', settings: { thinking: { type: 'adaptive' }, speed: 'fast' } },
    ])
  })
  test('fixed, default-only, or unknown tiers add no variant', () => {
    for (const service_tiers of [['priority'], ['default'], ['default', 'scale'], undefined])
      expect(
        toModel(provider, 'gpt', {}, undefined, undefined, undefined, {
          ...hints('openai-chat-completions'),
          service_tiers,
        }).variants,
      ).toEqual([])
  })
})
