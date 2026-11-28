import { useEffect, useState } from 'react'

import { getServiceAccounts } from '@/server/admin-data.functions'
import type { AdminTeamOption } from '@/types/api'
import type { UserView } from '@/types/live-api'

export interface CredentialOwnerOption {
  id: string
  label: string
}

export interface CredentialServiceAccountOption extends CredentialOwnerOption {
  teamId: string
}

/** The principals a platform admin can bind a credential to, named rather than by ID. */
export interface CredentialOwnerOptions {
  users: CredentialOwnerOption[]
  teams: CredentialOwnerOption[]
  serviceAccounts: CredentialServiceAccountOption[]
}

/** Active users, labelled for the owner picker. The `/mcp` loader builds these from its directory. */
export function toCredentialUserOptions(users: UserView[]): CredentialOwnerOption[] {
  const options: CredentialOwnerOption[] = []
  for (const user of users) {
    if (user.status === 'active') {
      options.push({ id: user.id, label: `${user.name} (${user.email})` })
    }
  }
  return options
}

/**
 * Names the credential owners once `enabled` turns on. Users and teams come from the page's
 * directory; only service accounts are fetched, when a platform admin opens Credentials. Returns
 * null until they load or when the fetch fails; the form then falls back to typed IDs.
 */
export function useCredentialOwnerOptions(
  enabled: boolean,
  users: CredentialOwnerOption[] | undefined,
  teams: AdminTeamOption[],
) {
  const [serviceAccounts, setServiceAccounts] = useState<CredentialServiceAccountOption[] | null>(
    null,
  )

  useEffect(() => {
    if (!enabled || serviceAccounts) return
    let cancelled = false
    void getServiceAccounts()
      .then((accounts) => {
        if (cancelled) return
        const active: CredentialServiceAccountOption[] = []
        for (const account of accounts.data.service_accounts) {
          if (account.status === 'active') {
            active.push({
              id: account.id,
              label: `${account.name} (${account.team_name})`,
              teamId: account.team_id,
            })
          }
        }
        setServiceAccounts(active)
      })
      .catch(() => {
        // Typed IDs still work, so a failed directory read only loses the pickers.
      })
    return () => {
      cancelled = true
    }
  }, [enabled, serviceAccounts])

  if (!serviceAccounts) return null
  return {
    users: users ?? [],
    teams: teams.map((team) => ({ id: team.id, label: team.name })),
    serviceAccounts,
  } satisfies CredentialOwnerOptions
}
