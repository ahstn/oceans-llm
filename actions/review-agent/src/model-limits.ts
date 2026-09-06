import type { EffectiveConfig } from './types'

export function oceansModelLimits(config: EffectiveConfig): {
  contextWindow: number
  maxTokens: number
} {
  const context = positiveLimit(config.model_context_window_tokens, 'context window')
  const output = positiveLimit(config.model_max_output_tokens, 'output limit')
  const input =
    config.model_input_window_tokens == null
      ? context
      : positiveLimit(config.model_input_window_tokens, 'input limit')
  // Keep review output bounded while respecting the gateway's route aggregate.
  const maxTokens = Math.min(output, 8192, Math.floor(context / 2))
  if (!maxTokens) throw new Error('Oceans model context window is too small for a review')
  return { contextWindow: Math.min(context, input + maxTokens), maxTokens }
}

function positiveLimit(value: unknown, name: string): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value <= 0) {
    throw new Error(
      `Oceans model ${name} is unavailable; configure authoritative route metadata before reviewing`,
    )
  }
  return value
}
