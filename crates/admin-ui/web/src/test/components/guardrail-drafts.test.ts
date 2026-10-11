import { act, renderHook } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import type { GuardrailPolicy } from '@/components/guardrails/decision-meta'
import { useGuardrailDrafts } from '@/components/guardrails/settings/model'

const base: GuardrailPolicy = {
  enabled: true,
  mode: 'audit',
  packs: ['core.shell'],
  managed_checks: [],
  stream_buffer_bytes: 4_194_304,
  stream_buffer_timeout_ms: 1_000,
  secret_redaction: { enabled: true, tiers: ['credentials'], disabled_rules: [] },
  scope: 'global',
  overridden_fields: [],
}

const policies = {
  default: base,
  model_routes: {
    fast: { ...base, scope: 'model_route:fast', overridden_fields: ['mode'] },
    slow: { ...base, scope: 'model_route:slow' },
  },
  mcp_servers: {},
  managed_checks: [],
  built_in_packs: [{ id: 'core.shell', version: '1.0.0' }],
}

describe('useGuardrailDrafts', () => {
  it('lets an undone edit inherit again but keeps fields the YAML already sets', () => {
    const { result } = renderHook(() => useGuardrailDrafts(policies))
    const toggleMode = (key: string, mode: 'audit' | 'deny') =>
      act(() => result.current.update(key, { ...result.current.drafts[key], mode }))

    toggleMode('model_route:slow', 'deny')
    toggleMode('model_route:slow', 'audit')
    toggleMode('model_route:fast', 'deny')
    toggleMode('model_route:fast', 'audit')

    expect(result.current.yaml).toContain('slow: {}')
    expect(result.current.yaml).toMatch(/fast:\n\s+mode: audit/)
  })
})
