import { useEffect, useState } from 'react'

import { getServiceAccounts, getUsers } from '@/server/admin-data.functions'

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

/**
 * Loads credential owners once `enabled` turns on, so the directory is read only when a platform
 * admin opens Credentials. Returns null until it loads or when it fails; the form then falls back
 * to typed IDs.
 */
export function useCredentialOwnerOptions(enabled: boolean) {
  const [options, setOptions] = useState<CredentialOwnerOptions | null>(null)

  useEffect(() => {
    if (!enabled || options) return
    let cancelled = false
    void Promise.all([getUsers(), getServiceAccounts()])
      .then(([users, accounts]) => {
        if (cancelled) return
        setOptions({
          users: users.data.users
            .filter((user) => user.status === 'active')
            .map((user) => ({ id: user.id, label: `${user.name} (${user.email})` })),
          teams: users.data.teams.map((team) => ({ id: team.id, label: team.name })),
          serviceAccounts: accounts.data.service_accounts
            .filter((account) => account.status === 'active')
            .map((account) => ({
              id: account.id,
              label: `${account.name} (${account.team_name})`,
              teamId: account.team_id,
            })),
        })
      })
      .catch(() => {
        // Typed IDs still work, so a failed directory read only loses the pickers.
      })
    return () => {
      cancelled = true
    }
  }, [enabled, options])

  return options
}
