import { describe, expect, it } from 'vitest'

import {
  listApiKeys,
  listModels,
  listRequestLogs,
  listTeams,
  listUsageCosts,
  listUsers,
} from '@/server/admin-data.server'

describe('server-side mock repositories', () => {
  it('returns stable API envelopes for phase-1 views', async () => {
    const [apiKeys, models, costs, logs, teams, users] = await Promise.all([
      listApiKeys(),
      listModels(),
      listUsageCosts(),
      listRequestLogs(),
      listTeams(),
      listUsers(),
    ])

    expect(apiKeys.data.items.length).toBeGreaterThan(0)
    expect(models.data.length).toBeGreaterThan(0)
    expect(costs.data.length).toBeGreaterThan(0)
    expect(logs.data.items.length).toBeGreaterThan(100)
    expect(teams.data.length).toBeGreaterThan(0)
    expect(users.data.length).toBeGreaterThan(0)
  })
})
