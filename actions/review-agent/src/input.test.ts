import { describe, expect, test } from 'bun:test'
import { buildOverrides, envInputReader, parseInputs } from './input'

describe('input parsing', () => {
  test('parses required inputs, booleans, integers, and default token', () => {
    const inputs = parseInputs(
      envInputReader({
        'INPUT_OCEANS-URL': 'https://oceans.example.test/',
        'INPUT_OCEANS-API-KEY': 'key',
        'INPUT_INLINE-REVIEW': 'true',
        'INPUT_TIMEOUT-MINUTES': '7',
        'INPUT_MAX-INLINE-COMMENTS': '12',
        'INPUT_DRY-RUN': 'yes',
        'INPUT_GITHUB-TOKEN': 'gh-token',
        GITHUB_TOKEN: 'gh-token',
      }),
    )

    expect(inputs.oceansUrl).toBe('https://oceans.example.test')
    expect(inputs.reportToOceans).toBe(true)
    expect(inputs.inlineReview).toBe(true)
    expect(inputs.timeoutMinutes).toBe(7)
    expect(inputs.maxInlineComments).toBe(12)
    expect(inputs.dryRun).toBe(true)
    expect(inputs.githubToken).toBe('gh-token')
  })

  test('builds only explicit config overrides', () => {
    const overrides = buildOverrides({
      reportToOceans: true,
      oceansUrl: 'https://oceans.example.test',
      oceansApiKey: 'key',
      modelId: 'gpt-5',
      inlineReview: false,
      timeoutMinutes: 20,
      dryRun: false,
      debug: false,
    })

    expect(overrides).toEqual({
      model_id: 'gpt-5',
      inline_review_enabled: false,
    })
  })

  test('standalone reviews need a direct model but no Oceans credentials', () => {
    const env = { 'INPUT_REPORT-TO-OCEANS': 'false', 'INPUT_MODEL-ID': 'openai/gpt-5' }
    const inputs = parseInputs(envInputReader(env))
    expect(inputs.reportToOceans).toBe(false)
    expect(inputs.modelMode).toBe('direct')
    expect(inputs.oceansUrl).toBe('')
    expect(inputs.oceansApiKey).toBe('')
    expect(() => parseInputs(envInputReader({ 'INPUT_REPORT-TO-OCEANS': 'false' }))).toThrow(
      'model-id',
    )
    expect(() => parseInputs(envInputReader({ ...env, 'INPUT_MODEL-MODE': 'oceans' }))).toThrow(
      'oceans-url',
    )
    expect(() =>
      parseInputs(envInputReader({ ...env, 'INPUT_REPORT-TO-OCEANS': 'invalid' })),
    ).toThrow('boolean')
  })
})
