import { describe, expect, it } from 'vitest'

import {
  aiBand,
  attributionReportInput,
  resolveAttributionTab,
} from '@/routes/observability/-attribution/insights'

import { platformAdminSession } from './auth-session'

describe('attribution report input', () => {
  const admin = platformAdminSession()

  it('scopes My work to the signed-in user whatever user filter the URL carries', () => {
    expect(
      attributionReportInput({ tab: 'my_work', days: 30, user_id: 'someone-else' }, admin),
    ).toEqual({ days: 30, user_id: admin.user.id })
  })

  it('keeps the user filter on other tabs and drops the tab itself', () => {
    expect(
      attributionReportInput({ tab: 'impact', days: 7, user_id: 'someone-else' }, admin),
    ).toEqual({ days: 7, user_id: 'someone-else' })
  })

  it('treats a turned-off My work tab like any other request', () => {
    const noMyWork = {
      ...admin,
      capabilities: { ...admin.capabilities, attribution_tabs: ['impact' as const] },
    }
    expect(resolveAttributionTab('my_work', noMyWork)).toBe('impact')
    expect(
      attributionReportInput({ tab: 'my_work', days: 30, user_id: 'someone-else' }, noMyWork),
    ).toEqual({ days: 30, user_id: 'someone-else' })
  })
})

describe('AI share bands', () => {
  it('keeps No AI for exactly zero AI lines', () => {
    expect(aiBand(0)).toBe('none')
    expect(aiBand(0.005)).toBe('light')
    expect(aiBand(0.5)).toBe('heavy')
    expect(aiBand(0.95)).toBe('agentic')
    expect(aiBand(1)).toBe('agentic')
  })
})
