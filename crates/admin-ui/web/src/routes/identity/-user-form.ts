import { sanitizeEntityTags } from '@/routes/identity/-entity-tag-utils'
import type { CreateUserInput, IdentityUsersPayload, UpdateUserInput, UserView } from '@/types/api'

type OidcProviders = IdentityUsersPayload['oidc_providers']
type OauthProviders = IdentityUsersPayload['oauth_providers']
type AuthFields = Pick<UpdateUserInput, 'auth_mode' | 'oidc_provider_key' | 'oauth_provider_key'>

/** The teams and SSO providers that the create and manage forms offer. */
export interface UserFormOptions {
  teams: IdentityUsersPayload['teams']
  oidcProviders: OidcProviders
  oauthProviders: OauthProviders
}

export const initialCreateUserForm: CreateUserInput = {
  name: '',
  email: '',
  auth_mode: 'password',
  global_role: 'user',
  team_id: null,
  team_role: null,
  oidc_provider_key: null,
  oauth_provider_key: null,
  tags: [],
}

export const initialUpdateUserForm: UpdateUserInput = {
  global_role: 'user',
  team_id: null,
  team_role: null,
  auth_mode: 'password',
  oidc_provider_key: null,
  oauth_provider_key: null,
  tags: [],
}

/** The manage form's starting values. An owner's team role stays out of the form. */
export function updateFormFor(user: UserView): UpdateUserInput {
  return {
    global_role: user.global_role,
    team_id: user.team_id,
    team_role: user.team_role === 'owner' ? null : user.team_role,
    auth_mode: user.auth_mode,
    oidc_provider_key:
      user.onboarding?.kind === 'oidc_sign_in' ? user.onboarding.provider_key : null,
    oauth_provider_key:
      user.onboarding?.kind === 'oauth_sign_in' ? user.onboarding.provider_key : null,
    tags: user.tags,
  }
}

/**
 * Switches the auth mode. An SSO mode keeps its chosen provider, or preselects the only configured
 * one, and the other mode's provider is cleared.
 */
export function withAuthMode<Form extends AuthFields>(
  form: Form,
  authMode: string,
  oidcProviders: OidcProviders,
  oauthProviders: OauthProviders,
): Form {
  return {
    ...form,
    auth_mode: authMode,
    oidc_provider_key:
      authMode === 'oidc' ? (form.oidc_provider_key ?? onlyProviderKey(oidcProviders)) : null,
    oauth_provider_key:
      authMode === 'oauth' ? (form.oauth_provider_key ?? onlyProviderKey(oauthProviders)) : null,
  }
}

function onlyProviderKey(providers: OidcProviders | OauthProviders) {
  return providers.length === 1 ? providers[0].key : null
}

/** Whether an SSO auth mode is chosen without a configured provider to complete it. */
export function isSsoProviderMissing(
  form: AuthFields,
  oidcProviders: OidcProviders,
  oauthProviders: OauthProviders,
) {
  return (
    (form.auth_mode === 'oidc' && (oidcProviders.length === 0 || !form.oidc_provider_key)) ||
    (form.auth_mode === 'oauth' && (oauthProviders.length === 0 || !form.oauth_provider_key))
  )
}

export function sanitizeCreateUserForm(form: CreateUserInput): CreateUserInput {
  return {
    ...form,
    name: form.name.trim(),
    email: form.email.trim(),
    team_id: form.team_id ?? null,
    team_role:
      form.team_id && form.team_role && form.team_role !== 'owner'
        ? form.team_role
        : form.team_id
          ? 'member'
          : null,
    oidc_provider_key: form.auth_mode === 'oidc' ? (form.oidc_provider_key ?? null) : null,
    oauth_provider_key: form.auth_mode === 'oauth' ? (form.oauth_provider_key ?? null) : null,
    tags: sanitizeEntityTags(form.tags),
  }
}

export function sanitizeUpdateForm(
  form: UpdateUserInput,
  user: UserView,
  oidcProviders: OidcProviders,
  oauthProviders: OauthProviders,
): UpdateUserInput {
  const update: UpdateUserInput = {
    global_role: form.global_role,
    tags: sanitizeEntityTags(form.tags),
  }

  if (user.team_role !== 'owner') {
    update.team_id = form.team_id ?? null
    update.team_role = form.team_id ? (form.team_role ?? 'member') : null
  }

  if (user.status === 'invited') {
    update.auth_mode = form.auth_mode
    update.oidc_provider_key = form.auth_mode === 'oidc' ? (form.oidc_provider_key ?? null) : null
    update.oauth_provider_key =
      form.auth_mode === 'oauth' ? (form.oauth_provider_key ?? null) : null
  }

  if (user.status === 'invited' && update.auth_mode === 'oidc') {
    const validProvider = oidcProviders.find(
      (provider) => provider.key === update.oidc_provider_key,
    )
    update.oidc_provider_key = validProvider ? update.oidc_provider_key : null
  }

  if (user.status === 'invited' && update.auth_mode === 'oauth') {
    const validProvider = oauthProviders.find(
      (provider) => provider.key === update.oauth_provider_key,
    )
    update.oauth_provider_key = validProvider ? update.oauth_provider_key : null
  }

  return update
}

export function sanitizeOnboardingUpdateForm(
  form: UpdateUserInput,
  user: UserView,
  oidcProviders: OidcProviders,
  oauthProviders: OauthProviders,
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

/** Whether the form's auth mode or provider differs from the user's pending onboarding. */
export function onboardingAuthChanged(user: UserView, form: UpdateUserInput) {
  if (form.auth_mode !== user.auth_mode) {
    return true
  }
  if (form.auth_mode === 'oidc') {
    return form.oidc_provider_key !== user.onboarding?.provider_key
  }
  if (form.auth_mode === 'oauth') {
    return form.oauth_provider_key !== user.onboarding?.provider_key
  }
  return false
}
