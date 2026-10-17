import { useState, type FormEvent } from 'react'
import { toast } from 'sonner'

import { createIdentityUser } from '@/server/admin-data.functions'
import {
  initialCreateUserForm,
  sanitizeCreateUserForm,
  withAuthMode,
} from '@/routes/identity/-user-form'
import type { UsersTransition } from '@/routes/identity/-use-users-transition'
import type { CreateUserInput, CreateUserResult, IdentityUsersPayload } from '@/types/api'

export type CreateUserState = ReturnType<typeof useCreateUser>

/** The create-user form and the onboarding URL it produces. */
export function useCreateUser({
  oidcProviders,
  oauthProviders,
  transition,
}: {
  oidcProviders: IdentityUsersPayload['oidc_providers']
  oauthProviders: IdentityUsersPayload['oauth_providers']
  transition: UsersTransition
}) {
  const [form, setForm] = useState<CreateUserInput>(initialCreateUserForm)
  const [result, setResult] = useState<CreateUserResult | null>(null)

  function setAuthMode(authMode: string) {
    setForm((current) => withAuthMode(current, authMode, oidcProviders, oauthProviders))
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()

    transition.run(async () => {
      const response = await createIdentityUser({ data: sanitizeCreateUserForm(form) })
      setResult(response.data)
      toast.success(
        response.data.kind === 'password_invite'
          ? 'Password invite created'
          : 'SSO sign-in URL created',
      )
      await transition.refreshUsers()
    })
  }

  function clearResult() {
    setResult(null)
  }

  function reset() {
    setForm(initialCreateUserForm)
    setResult(null)
  }

  return { form, setForm, result, setAuthMode, submit, clearResult, reset }
}
