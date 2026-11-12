import { describe, expect, test } from 'bun:test'
import { Redactor } from './redaction'
import type { ReviewResult } from './types'

describe('review output redaction', () => {
  test('removes raw and checkout-encoded credentials before publication', () => {
    const token = 'ghs_example-secret'
    const encoded = Buffer.from(`x-access-token:${token}`).toString('base64')
    const result: ReviewResult = {
      summary: `raw ${token}; encoded ${encoded}`,
      findings: [{ path: 'src/example.ts', line: 1, message: `credential: ${token}` }],
      metrics: { files_changed: 1 },
      degradedFeatures: [`provider-${token}`],
    }

    const redacted = new Redactor([token]).reviewResult(result)

    expect(redacted.summary).toBe('raw ***; encoded ***')
    expect(redacted.findings[0]?.message).toBe('credential: ***')
    expect(redacted.degradedFeatures).toEqual(['provider-***'])
    expect(redacted.metrics).toEqual({ files_changed: 1 })
    expect(result.summary).toContain(token)
  })
})
