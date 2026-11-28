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
 * One owner list: `loading` while its read is in flight (the picker waits, so nothing can be typed
 * in the meantime), `failed` when the read failed (that owner then takes a typed ID), or the
 * loaded options.
 */
export type OwnerList<T> = T[] | 'loading' | 'failed'

/** The principals a platform admin can bind a credential to, named rather than by ID. */
export interface CredentialOwnerOptions {
  users: OwnerList<CredentialOwnerOption>
  teams: OwnerList<CredentialOwnerOption>
  serviceAccounts: OwnerList<CredentialServiceAccountOption>
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
  const [users, setUsers] = useState<OwnerList<CredentialOwnerOption>>('loading')
  const [serviceAccounts, setServiceAccounts] =
    useState<OwnerList<CredentialServiceAccountOption>>('loading')
  // Each list is read once: a loaded or failed list stays as it is.
  const needsUsers = enabled && scopeKind === 'user' && users === 'loading'
  const needsServiceAccounts =
    enabled && scopeKind === 'service_account' && serviceAccounts === 'loading'

  useEffect(() => {
    if (!needsUsers) return
    let cancelled = false
    void getUsers()
      .then((response) => {
        if (!cancelled) setUsers(activeUserOptions(response.data.users))
      })
      .catch(() => {
        // Typed IDs still work, so a failed directory read only loses the picker.
        if (!cancelled) setUsers('failed')
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
        if (!cancelled) setServiceAccounts('failed')
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
