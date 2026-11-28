import { useEffect, useState } from 'react'

import { getServiceAccounts, getUsers } from '@/server/admin-data.functions'
import type { AdminTeamOption } from '@/types/api'

export interface CredentialOwnerOption {
  id: string
  label: string
}

export interface CredentialServiceAccountOption extends CredentialOwnerOption {
  teamId: string
}

/**
 * The principals a platform admin can bind a credential to, named rather than by ID. A list is
 * null until it loads, or when its read fails; that owner then takes a typed ID.
 */
export interface CredentialOwnerOptions {
  users: CredentialOwnerOption[] | null
  teams: CredentialOwnerOption[] | null
  serviceAccounts: CredentialServiceAccountOption[] | null
}

type OwnerScopeKind = 'user' | 'team' | 'service_account'

/**
 * Names credential owners for the scope the form has chosen, once `enabled` turns on. Each list
 * loads the first time its scope is picked: users for a user binding, service accounts for a
 * service account binding. Teams come from the page's directory, so they need no read.
 */
export function useCredentialOwnerOptions(
  enabled: boolean,
  scopeKind: OwnerScopeKind,
  teams: AdminTeamOption[],
): CredentialOwnerOptions | null {
  const [users, setUsers] = useState<CredentialOwnerOption[] | null>(null)
  const [serviceAccounts, setServiceAccounts] = useState<CredentialServiceAccountOption[] | null>(
    null,
  )
  const needsUsers = enabled && scopeKind === 'user' && !users
  const needsServiceAccounts = enabled && scopeKind === 'service_account' && !serviceAccounts

  useEffect(() => {
    if (!needsUsers) return
    let cancelled = false
    void getUsers()
      .then((response) => {
        if (!cancelled) setUsers(activeUserOptions(response.data.users))
      })
      .catch(() => {
        // Typed IDs still work, so a failed directory read only loses the picker.
      })
    return () => {
      cancelled = true
    }
  }, [needsUsers])

  useEffect(() => {
    if (!needsServiceAccounts) return
    let cancelled = false
    void getServiceAccounts()
      .then((response) => {
        if (!cancelled)
          setServiceAccounts(activeServiceAccountOptions(response.data.service_accounts))
      })
      .catch(() => {
        // Typed IDs still work, so a failed directory read only loses the picker.
      })
    return () => {
      cancelled = true
    }
  }, [needsServiceAccounts])

  if (!enabled) return null
  return {
    users,
    teams: teams.map((team) => ({ id: team.id, label: team.name })),
    serviceAccounts,
  }
}

type UsersResponse = Awaited<ReturnType<typeof getUsers>>
type ServiceAccountsResponse = Awaited<ReturnType<typeof getServiceAccounts>>

function activeUserOptions(users: UsersResponse['data']['users']) {
  const options: CredentialOwnerOption[] = []
  for (const user of users) {
    if (user.status === 'active')
      options.push({ id: user.id, label: `${user.name} (${user.email})` })
  }
  return options
}

function activeServiceAccountOptions(
  accounts: ServiceAccountsResponse['data']['service_accounts'],
) {
  const options: CredentialServiceAccountOption[] = []
  for (const account of accounts) {
    if (account.status === 'active') {
      options.push({
        id: account.id,
        label: `${account.name} (${account.team_name})`,
        teamId: account.team_id,
      })
    }
  }
  return options
}
