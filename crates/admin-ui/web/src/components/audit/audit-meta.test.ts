import { describe, expect, it } from 'vitest'

import { actionVariant, actionVerbLabel, targetTypeLabel } from './audit-meta'

describe('audit labels', () => {
  it('names target types with product acronyms', () => {
    expect(targetTypeLabel('api_key')).toBe('API key')
    expect(targetTypeLabel('mcp_credential_binding')).toBe('MCP credential binding')
    expect(targetTypeLabel('review_agent_repository')).toBe('Review agent repository')
  })

  it('reads the verb from the action and tones it by effect', () => {
    expect(actionVerbLabel('api_key.secret_revealed')).toBe('Secret revealed')
    expect(actionVariant('api_key.revoked')).toBe('destructive')
    expect(actionVariant('team.created')).toBe('success')
    expect(actionVariant('budget.set')).toBe('secondary')
  })
})
