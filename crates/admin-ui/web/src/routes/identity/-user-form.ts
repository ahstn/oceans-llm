import type { IdentityUsersPayload, UpdateUserInput, UserView } from '@/types/api'

export function sanitizeOnboardingUpdateForm(
  form: UpdateUserInput,
  user: UserView,
  oidcProviders: IdentityUsersPayload['oidc_providers'],
  oauthProviders: IdentityUsersPayload['oauth_providers'],
): UpdateUserInput {
  const update: UpdateUserInput = {
    global_role: user.global_role,
    auth_mode: form.auth_mode,
    oidc_provider_key: form.auth_mode === 'oidc' ? (form.oidc_provider_key ?? null) : null,
    oauth_provider_key: form.auth_mode === 'oauth' ? (form.oauth_provider_key ?? null) : null,
  }

  if (user.team_role !== 'owner') {
    update.team_id = user.team_id ?? null
    update.team_role = user.team_id ? (user.team_role ?? 'member') : null
  }

  if (update.auth_mode === 'oidc') {
    const validProvider = oidcProviders.find(
      (provider) => provider.key === update.oidc_provider_key,
    )
    update.oidc_provider_key = validProvider ? update.oidc_provider_key : null
  }

  if (update.auth_mode === 'oauth') {
    const validProvider = oauthProviders.find(
      (provider) => provider.key === update.oauth_provider_key,
    )
    update.oauth_provider_key = validProvider ? update.oauth_provider_key : null
  }

  return update
}
