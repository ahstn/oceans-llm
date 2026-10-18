import { useState } from 'react'
import { toast } from 'sonner'

import { resetIdentityUserOnboarding, updateIdentityUser } from '@/server/admin-data.functions'
import {
  getErrorMessage,
  type IdentityTransition,
} from '@/routes/identity/-use-identity-transition'
import { onboardingAuthChanged, sanitizeOnboardingUpdateForm } from '@/routes/identity/-user-form'
import type { CreateUserResult, IdentityUsersPayload, UpdateUserInput, UserView } from '@/types/api'

export type OnboardingReset = ReturnType<typeof useOnboardingReset>

/**
 * Reissues a user's onboarding URL. An invited user's unsaved auth change is saved first, so the
 * new URL uses it. Callers key the owning component by user, so the URL is dropped on a switch.
 */
export function useOnboardingReset({
  user,
  updateForm,
  oidcProviders,
  oauthProviders,
  transition,
}: {
  user: UserView
  updateForm: UpdateUserInput
  oidcProviders: IdentityUsersPayload['oidc_providers']
  oauthProviders: IdentityUsersPayload['oauth_providers']
  transition: IdentityTransition
}) {
  const [onboardingResult, setOnboardingResult] = useState<CreateUserResult | null>(null)

  function resetOnboarding() {
    let savedOnboardingAuth = false
    transition.run(
      async () => {
        setOnboardingResult(null)
        if (user.status === 'invited' && onboardingAuthChanged(user, updateForm)) {
          await updateIdentityUser({
            data: {
              userId: user.id,
              input: sanitizeOnboardingUpdateForm(updateForm, user, oidcProviders, oauthProviders),
            },
          })
          savedOnboardingAuth = true
        }
        const response = await resetIdentityUserOnboarding({ data: { userId: user.id } })
        setOnboardingResult(response.data)
        toast.success(
          response.data.kind === 'password_invite'
            ? 'Password invite regenerated'
            : 'SSO sign-in URL regenerated',
        )
        await transition.refresh()
      },
      async (error) => {
        if (savedOnboardingAuth) {
          await transition.refresh()
        }
        toast.error(
          savedOnboardingAuth
            ? `Auth method was saved, but reset failed: ${getErrorMessage(error)}`
            : getErrorMessage(error),
        )
      },
    )
  }

  return { onboardingResult, resetOnboarding }
}
