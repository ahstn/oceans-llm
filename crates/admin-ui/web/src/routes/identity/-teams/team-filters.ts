import type { EntityTag } from '@/routes/identity/-entity-tag-utils'
import type { TeamManagementView } from '@/types/api'

export type TeamStatusFilter = 'all' | 'active' | 'inactive'

/** What narrows the team list: free text, status, teams without admins, and tag pairs. */
export interface TeamFilters {
  query: string
  status: TeamStatusFilter
  withoutAdmins: boolean
  tags: EntityTag[]
}

export const emptyTeamFilters: TeamFilters = {
  query: '',
  status: 'all',
  withoutAdmins: false,
  tags: [],
}

export function tagFilterId(tag: EntityTag) {
  return `${tag.key}:${tag.value}`
}

export function hasActiveFilters(filters: TeamFilters) {
  return (
    filters.query.trim() !== '' ||
    filters.status !== 'all' ||
    filters.withoutAdmins ||
    filters.tags.length > 0
  )
}

/** Adds the tag filter, or removes it when it is already applied. */
export function toggleTagFilter(filters: TeamFilters, tag: EntityTag): TeamFilters {
  const id = tagFilterId(tag)
  const applied = filters.tags.some((entry) => tagFilterId(entry) === id)
  return {
    ...filters,
    tags: applied
      ? filters.tags.filter((entry) => tagFilterId(entry) !== id)
      : [...filters.tags, tag],
  }
}

/**
 * Teams that pass every filter. Tag filters on the same key match any of their values; filters on
 * different keys must all match, so `env:prod` plus `env:staging` widens and `owner:infra` narrows.
 */
export function filterTeams(teams: TeamManagementView[], filters: TeamFilters) {
  const query = filters.query.trim().toLowerCase()
  const valuesByKey = new Map<string, Set<string>>()
  for (const tag of filters.tags) {
    const values = valuesByKey.get(tag.key) ?? new Set<string>()
    values.add(tag.value)
    valuesByKey.set(tag.key, values)
  }

  return teams.filter((team) => {
    if (filters.status === 'active' && team.status !== 'active') return false
    if (filters.status === 'inactive' && team.status === 'active') return false
    if (filters.withoutAdmins && team.admins.length > 0) return false
    if (
      query &&
      !team.name.toLowerCase().includes(query) &&
      !team.key.toLowerCase().includes(query)
    ) {
      return false
    }
    for (const [key, values] of valuesByKey) {
      if (!team.tags.some((tag) => tag.key === key && values.has(tag.value))) return false
    }
    return true
  })
}

/** Every distinct tag pair across the teams, sorted by key and then value. */
export function teamTagOptions(teams: TeamManagementView[]): EntityTag[] {
  const byId = new Map<string, EntityTag>()
  for (const team of teams) {
    for (const tag of team.tags) byId.set(tagFilterId(tag), { key: tag.key, value: tag.value })
  }
  return [...byId.values()].sort(
    (a, b) => a.key.localeCompare(b.key) || a.value.localeCompare(b.value),
  )
}
