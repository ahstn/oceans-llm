import { describe, expect, it } from 'vitest'

import { draftFrom, scopeYaml } from '@/components/guardrails/config-yaml'
import type { GuardrailPolicy, PolicyScope } from '@/components/guardrails/decision-meta'

const base: GuardrailPolicy = {
  enabled: true,
  mode: 'audit',
  packs: ['core.shell', 'core.git'],
  managed_checks: [],
  stream_buffer_bytes: 4_194_304,
  secret_redaction: {
    enabled: true,
    tiers: ['provider_tokens', 'credentials'],
    disabled_rules: [],
  },
  scope: 'global',
}

describe('scopeYaml', () => {
  it('writes the default block in full', () => {
    const scope: PolicyScope = {
      key: 'global',
      kind: 'global',
      name: 'Global default',
      policy: base,
    }

    expect(scopeYaml(scope, draftFrom(base), draftFrom(base))).toBe(
      [
        'guardrails:',
        '  default:',
        '    enabled: true',
        '    mode: audit',
        '    packs: [core.shell, core.git]',
        '    managed_checks: []',
        '    stream_buffer_bytes: 4194304',
        '    secret_redaction:',
        '      enabled: true',
        '      tiers: [provider_tokens, credentials]',
        '      disabled_rules: []',
        '',
      ].join('\n'),
    )
  })

  it('writes only the fields an override changes', () => {
    const policy = { ...base, scope: 'model_route:fast/openai/gpt-5' }
    const scope: PolicyScope = {
      key: policy.scope,
      kind: 'model',
      name: 'fast/openai/gpt-5',
      policy,
    }
    const draft = draftFrom(policy)
    draft.mode = 'deny'
    draft.redaction.tiers = ['provider_tokens', 'credentials', 'generic']

    expect(scopeYaml(scope, draft, draftFrom(base))).toBe(
      [
        'guardrails:',
        '  model_routes:',
        '    fast/openai/gpt-5:',
        '      mode: deny',
        '      secret_redaction:',
        '        tiers: [provider_tokens, credentials, generic]',
        '',
      ].join('\n'),
    )
  })

  it('writes an empty override when a scope matches the default', () => {
    const policy = { ...base, scope: 'mcp_server:github' }
    const scope: PolicyScope = { key: policy.scope, kind: 'mcp', name: 'github', policy }

    expect(scopeYaml(scope, draftFrom(policy), draftFrom(base))).toBe(
      'guardrails:\n  mcp_servers:\n    github: {}\n',
    )
  })

  it('quotes list values YAML would misread', () => {
    const policy = { ...base, scope: 'mcp_server:github' }
    const scope: PolicyScope = { key: policy.scope, kind: 'mcp', name: 'github', policy }
    const draft = draftFrom(policy)
    draft.managed_checks = ['armor, prod', 'bedrock']

    expect(scopeYaml(scope, draft, draftFrom(base))).toBe(
      'guardrails:\n  mcp_servers:\n    github:\n      managed_checks: ["armor, prod", bedrock]\n',
    )
  })

  it('quotes names YAML would load as booleans, nulls, or numbers', () => {
    const policy = { ...base, scope: 'mcp_server:true' }
    const scope: PolicyScope = { key: policy.scope, kind: 'mcp', name: 'true', policy }
    const draft = draftFrom(policy)
    draft.managed_checks = ['null', '123', 'armor']

    expect(scopeYaml(scope, draft, draftFrom(base))).toBe(
      'guardrails:\n  mcp_servers:\n    "true":\n      managed_checks: ["null", "123", armor]\n',
    )
  })

  it('keeps a field set back to the inherited value so the merge replaces it', () => {
    const policy = { ...base, mode: 'deny' as const, scope: 'mcp_server:github' }
    const scope: PolicyScope = { key: policy.scope, kind: 'mcp', name: 'github', policy }
    const draft = draftFrom(policy)
    draft.mode = 'audit'

    expect(scopeYaml(scope, draft, draftFrom(base))).toBe(
      'guardrails:\n  mcp_servers:\n    github:\n      mode: audit\n',
    )
  })
})
