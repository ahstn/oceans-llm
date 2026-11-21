import { describe, expect, it } from 'vitest'

import {
  emptyTeamFilters,
  filterTeams,
  hasActiveFilters,
  teamTagOptions,
  toggleTagFilter,
} from '@/routes/identity/-teams/team-filters'
import type { TeamManagementView } from '@/types/api'

function team(key: string, overrides: Partial<TeamManagementView> = {}): TeamManagementView {
  return {
    id: `team_${key}`,
    key,
    name: key,
    status: 'active',
    tags: [],
    admins: [],
    members: [],
    member_count: 0,
    ...overrides,
  }
}

const admin = { id: 'user_1', name: 'Jane', email: 'jane@example.com', status: 'active' }
const teams = [
  team('platform', {
    name: 'Platform',
    admins: [admin],
    tags: [
      { key: 'env', value: 'prod' },
      { key: 'owner', value: 'infra' },
    ],
  }),
  team('staging-tools', { tags: [{ key: 'env', value: 'staging' }] }),
  team('sandbox', { status: 'inactive', tags: [{ key: 'env', value: 'sandbox' }] }),
]

describe('filterTeams', () => {
  it('matches any value of one tag key, and every key', () => {
    const prodOrStaging = toggleTagFilter(
      toggleTagFilter(emptyTeamFilters, { key: 'env', value: 'prod' }),
      { key: 'env', value: 'staging' },
    )
    expect(filterTeams(teams, prodOrStaging).map((t) => t.key)).toEqual([
      'platform',
      'staging-tools',
    ])

    const prodOwnedByInfra = toggleTagFilter(prodOrStaging, { key: 'owner', value: 'infra' })
    expect(filterTeams(teams, prodOwnedByInfra).map((t) => t.key)).toEqual(['platform'])
  })

  it('searches names and keys, and filters by status and missing admins', () => {
    expect(filterTeams(teams, { ...emptyTeamFilters, query: 'PLAT' }).map((t) => t.key)).toEqual([
      'platform',
    ])
    expect(filterTeams(teams, { ...emptyTeamFilters, query: 'tools' }).map((t) => t.key)).toEqual([
      'staging-tools',
    ])
    expect(
      filterTeams(teams, { ...emptyTeamFilters, status: 'inactive' }).map((t) => t.key),
    ).toEqual(['sandbox'])
    expect(
      filterTeams(teams, { ...emptyTeamFilters, withoutAdmins: true }).map((t) => t.key),
    ).toEqual(['staging-tools', 'sandbox'])
  })

  it('toggles a tag filter off when it is applied again', () => {
    const tag = { key: 'env', value: 'prod' }
    const applied = toggleTagFilter(emptyTeamFilters, tag)
    expect(hasActiveFilters(applied)).toBe(true)
    expect(toggleTagFilter(applied, tag)).toEqual(emptyTeamFilters)
    expect(hasActiveFilters(emptyTeamFilters)).toBe(false)
  })
})

describe('team list summaries', () => {
  it('lists each tag pair once, sorted', () => {
    expect(teamTagOptions(teams)).toEqual([
      { key: 'env', value: 'prod' },
      { key: 'env', value: 'sandbox' },
      { key: 'env', value: 'staging' },
      { key: 'owner', value: 'infra' },
    ])
  })
})
