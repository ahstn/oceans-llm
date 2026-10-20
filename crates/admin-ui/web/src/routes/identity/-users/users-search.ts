import {
  ChartLineData01Icon,
  Configuration01Icon,
  ShieldKeyIcon,
  UserCircleIcon,
} from '@hugeicons/core-free-icons'

export const userDetailsSections = [
  { id: 'overview', label: 'Overview', icon: UserCircleIcon },
  { id: 'configuration', label: 'Configuration', icon: Configuration01Icon },
  { id: 'auth', label: 'Auth & onboarding', icon: ShieldKeyIcon },
  { id: 'provider-configuration', label: 'Provider Configuration', icon: ShieldKeyIcon },
  { id: 'usage', label: 'Usage', icon: ChartLineData01Icon },
] as const

export type UserDetailsSection = (typeof userDetailsSections)[number]['id']

export type UsersSearch = ReturnType<typeof normalizeUserSearch>

/** The user whose details dialog is open, and its section. Unknown sections fall back to overview. */
export function normalizeUserSearch(search: Record<string, unknown>) {
  const section = typeof search.user_section === 'string' ? search.user_section : 'overview'

  return {
    user_id:
      typeof search.user_id === 'string' && search.user_id.length > 0 ? search.user_id : undefined,
    user_section: isUserDetailsSection(section) ? section : 'overview',
  }
}

function isUserDetailsSection(value: string): value is UserDetailsSection {
  return userDetailsSections.some((section) => section.id === value)
}
