import { useEffect, useState, type FormEvent } from 'react'
import { useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import {
  deactivateIdentityUser,
  reactivateIdentityUser,
  updateIdentityUser,
} from '@/server/admin-data.functions'
import { useProviderCredentials } from '@/routes/identity/-users/use-provider-credentials'
import type { IdentityTransition } from '@/routes/identity/-use-identity-transition'
import {
  initialUpdateUserForm,
  sanitizeUpdateForm,
  updateFormFor,
  withAuthMode,
} from '@/routes/identity/-user-form'
import type { UserDetailsSection } from '@/routes/identity/-users/users-search'
import type { IdentityUsersPayload, UpdateUserInput, UserView } from '@/types/api'

export type UserDetails = ReturnType<typeof useUserDetails>

/**
 * State and actions for the `Manage user` dialog. They live above the per-user form, so provider
 * token drafts survive switching users. The manage form follows the user record, so it resets to
 * the saved values whenever the user list refreshes.
 */
export function useUserDetails({
  user,
  oidcProviders,
  oauthProviders,
  transition,
}: {
  user: UserView | null
  oidcProviders: IdentityUsersPayload['oidc_providers']
  oauthProviders: IdentityUsersPayload['oauth_providers']
  transition: IdentityTransition
}) {
  const router = useRouter()
  const [updateForm, setUpdateForm] = useState<UpdateUserInput>(initialUpdateUserForm)

  useEffect(() => {
    setUpdateForm(user ? updateFormFor(user) : initialUpdateUserForm)
  }, [user])

  const credentials = useProviderCredentials(user?.id ?? null, transition)

  function setUpdateAuthMode(authMode: string) {
    setUpdateForm((current) => withAuthMode(current, authMode, oidcProviders, oauthProviders))
  }

  function showSection(section: UserDetailsSection) {
    if (!user) {
      return
    }

    void router.navigate({
      to: '/identity/users',
      search: { user_id: user.id, user_section: section },
    })
  }

  function close() {
    void router.navigate({
      to: '/identity/users',
      search: {},
    })
    setUpdateForm(initialUpdateUserForm)
    credentials.clearAll()
  }

  /** Runs a change that ends the edit: on success the list refreshes and the dialog closes. */
  function runAndClose(action: (target: UserView) => Promise<unknown>, successMessage: string) {
    if (!user) {
      return
    }

    transition.run(async () => {
      await action(user)
      toast.success(successMessage)
      await transition.refresh()
      close()
    })
  }

  function saveUser(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    runAndClose(
      (target) =>
        updateIdentityUser({
          data: {
            userId: target.id,
            input: sanitizeUpdateForm(updateForm, target, oidcProviders, oauthProviders),
          },
        }),
      'User updated',
    )
  }

  function deactivateUser() {
    runAndClose(
      (target) => deactivateIdentityUser({ data: { userId: target.id } }),
      'User deactivated',
    )
  }

  function reactivateUser() {
    runAndClose(
      (target) => reactivateIdentityUser({ data: { userId: target.id } }),
      'User reactivated',
    )
  }

  return {
    updateForm,
    setUpdateForm,
    setUpdateAuthMode,
    credentials,
    showSection,
    close,
    saveUser,
    deactivateUser,
    reactivateUser,
  }
}
