import { expect, test } from 'bun:test'
import { oceansModelLimits } from './model-limits'

test('uses smaller gateway limits and respects a separate input ceiling', () => {
  expect(
    oceansModelLimits({
      model_context_window_tokens: 4096,
      model_input_window_tokens: 2000,
      model_max_output_tokens: 512,
    }),
  ).toEqual({ contextWindow: 2512, maxTokens: 512 })
})

test('refuses to invent limits for an unknown gateway model', () => {
  expect(() => oceansModelLimits({})).toThrow('context window is unavailable')
  expect(() => oceansModelLimits({ model_context_window_tokens: 4096 })).toThrow(
    'output limit is unavailable',
  )
})
