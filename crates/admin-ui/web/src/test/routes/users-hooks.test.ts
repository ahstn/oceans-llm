import { act, renderHook, waitFor } from '@testing-library/react'
import type { FormEvent } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useCreateUser } from '@/routes/identity/-use-create-user'
import { useOnboardingReset } from '@/routes/identity/-use-onboarding-reset'
import { useProviderCredentials } from '@/routes/identity/-use-provider-credentials'
import { useUserDetails } from '@/routes/identity/-use-user-details'
import { useUsersTransition } from '@/routes/identity/-use-users-transition'
import { initialUpdateUserForm, updateFormFor } from '@/routes/identity/-user-form'
import type { IdentityUsersPayload, UpdateUserInput, UserView } from '@/types/api'

const mocks = vi.hoisted(() => ({
  router: {
    invalidate: vi.fn(async () => {}),
    navigate: vi.fn(async () => {}),
  },
  toast: { success: vi.fn(), error: vi.fn() },
  createIdentityUser: vi.fn(),
  updateIdentityUser: vi.fn(),
  resetIdentityUserOnboarding: vi.fn(),
  deactivateIdentityUser: vi.fn(),
  reactivateIdentityUser: vi.fn(),
  saveIdentityUserProviderCredential: vi.fn(),
  removeIdentityUserProviderCredential: vi.fn(),
}))

vi.mock('@tanstack/react-router', () => ({ useRouter: () => mocks.router }))
vi.mock('sonner', () => ({ toast: mocks.toast }))
vi.mock('@/server/admin-data.functions', () => ({
  createIdentityUser: mocks.createIdentityUser,
  updateIdentityUser: mocks.updateIdentityUser,
  resetIdentityUserOnboarding: mocks.resetIdentityUserOnboarding,
  deactivateIdentityUser: mocks.deactivateIdentityUser,
  reactivateIdentityUser: mocks.reactivateIdentityUser,
  saveIdentityUserProviderCredential: mocks.saveIdentityUserProviderCredential,
  removeIdentityUserProviderCredential: mocks.removeIdentityUserProviderCredential,
}))

const oidcProviders: IdentityUsersPayload['oidc_providers'] = [
  { id: 'oidc_1', key: 'okta', label: 'Okta' },
]
const oauthProviders: IdentityUsersPayload['oauth_providers'] = [
  { id: 'oauth_1', key: 'github', label: 'GitHub' },
  { id: 'oauth_2', key: 'gitlab', label: 'GitLab' },
]

function invitedUser(overrides: Partial<UserView> = {}): UserView {
  return {
    id: 'user_1',
    name: 'Jane Operator',
    email: 'jane@example.com',
    auth_mode: 'password',
    global_role: 'user',
    team_id: null,
    team_name: null,
    team_role: null,
    request_logging_enabled: true,
    status: 'invited',
    tags: [],
    onboarding: null,
    ...overrides,
  }
}

const passwordInvite = {
  kind: 'password_invite' as const,
  invite_url: 'http://example.test/invite/user-1',
  expires_at: '2026-03-14T12:00:00Z',
  user: invitedUser(),
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}

function submitEvent() {
  const preventDefault = vi.fn()
  return { event: { preventDefault } as unknown as FormEvent<HTMLFormElement>, preventDefault }
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('useUsersTransition', () => {
  it('stays pending until the action settles', async () => {
    const request = deferred<void>()
    const { result } = renderHook(() => useUsersTransition())

    act(() => result.current.run(() => request.promise))
    expect(result.current.isPending).toBe(true)

    await act(async () => request.resolve())
    expect(result.current.isPending).toBe(false)
  })

  it('reports a failed action as an error toast', async () => {
    const { result } = renderHook(() => useUsersTransition())

    act(() =>
      result.current.run(async () => {
        throw new Error('Gateway unavailable')
      }),
    )

    await waitFor(() => expect(mocks.toast.error).toHaveBeenCalledWith('Gateway unavailable'))
  })

  it('hands a failure to onError instead of the default toast', async () => {
    const failure = new Error('reset failed')
    const onError = vi.fn()
    const { result } = renderHook(() => useUsersTransition())

    act(() =>
      result.current.run(async () => {
        throw failure
      }, onError),
    )

    await waitFor(() => expect(onError).toHaveBeenCalledWith(failure))
    expect(mocks.toast.error).not.toHaveBeenCalled()
  })
})

describe('useCreateUser', () => {
  function renderCreateUser() {
    return renderHook(() => {
      const transition = useUsersTransition()
      return useCreateUser({ oidcProviders, oauthProviders, transition })
    })
  }

  it('preselects the only provider for an SSO mode and clears the other mode', () => {
    const { result } = renderCreateUser()

    act(() => result.current.setAuthMode('oidc'))
    expect(result.current.form).toMatchObject({
      auth_mode: 'oidc',
      oidc_provider_key: 'okta',
      oauth_provider_key: null,
    })

    act(() => result.current.setAuthMode('oauth'))
    expect(result.current.form).toMatchObject({
      auth_mode: 'oauth',
      oidc_provider_key: null,
      oauth_provider_key: null,
    })
  })

  it('creates the user from the trimmed form and keeps the generated URL', async () => {
    mocks.createIdentityUser.mockResolvedValue({ data: passwordInvite })
    const { result } = renderCreateUser()
    const { event, preventDefault } = submitEvent()

    act(() =>
      result.current.setForm((current) => ({
        ...current,
        name: '  Jane Operator ',
        email: ' jane@example.com ',
        team_id: 'team_1',
        tags: [
          { key: ' cost-center ', value: 'platform ' },
          { key: ' ', value: '' },
        ],
      })),
    )
    await act(async () => result.current.submit(event))

    expect(preventDefault).toHaveBeenCalled()
    expect(mocks.createIdentityUser).toHaveBeenCalledWith({
      data: {
        name: 'Jane Operator',
        email: 'jane@example.com',
        auth_mode: 'password',
        global_role: 'user',
        team_id: 'team_1',
        team_role: 'member',
        oidc_provider_key: null,
        oauth_provider_key: null,
        tags: [{ key: 'cost-center', value: 'platform' }],
      },
    })
    await waitFor(() => expect(result.current.result).toEqual(passwordInvite))
    expect(mocks.toast.success).toHaveBeenCalledWith('Password invite created')
    expect(mocks.router.invalidate).toHaveBeenCalledTimes(1)

    act(() => result.current.reset())
    expect(result.current.result).toBeNull()
    expect(result.current.form.name).toBe('')
  })
})

describe('useProviderCredentials', () => {
  function renderCredentials() {
    return renderHook(
      ({ userId }: { userId: string | null }) => {
        const transition = useUsersTransition()
        return useProviderCredentials(userId, transition)
      },
      { initialProps: { userId: 'user_1' as string | null } },
    )
  }

  it('keeps a separate draft for each user', () => {
    const { result, rerender } = renderCredentials()

    act(() => result.current.updateToken('user_1', 'copilot', 'first-user-token'))
    rerender({ userId: 'user_2' })
    expect(result.current.tokens).toEqual({})

    act(() => result.current.updateToken('user_2', 'copilot', 'second-user-token'))
    rerender({ userId: 'user_1' })
    expect(result.current.tokens).toEqual({ copilot: 'first-user-token' })

    act(() => result.current.clearAll())
    expect(result.current.tokens).toEqual({})
  })

  it('rejects a blank token without calling the gateway', () => {
    const { result } = renderCredentials()

    act(() => result.current.updateToken('user_1', 'copilot', '   '))
    act(() => result.current.save('copilot'))

    expect(mocks.toast.error).toHaveBeenCalledWith('Enter a GitHub token')
    expect(mocks.saveIdentityUserProviderCredential).not.toHaveBeenCalled()
  })

  it('saves the trimmed token, then clears the draft it sent', async () => {
    mocks.saveIdentityUserProviderCredential.mockResolvedValue({ data: { configured: true } })
    const { result } = renderCredentials()

    act(() => result.current.updateToken('user_1', 'copilot', ' gho_token '))
    await act(async () => result.current.save('copilot'))

    expect(mocks.saveIdentityUserProviderCredential).toHaveBeenCalledWith({
      data: { userId: 'user_1', providerKey: 'copilot', token: 'gho_token' },
    })
    await waitFor(() => expect(result.current.tokens).toEqual({ copilot: '' }))
    expect(mocks.toast.success).toHaveBeenCalledWith('Provider token saved')
    expect(mocks.router.invalidate).toHaveBeenCalledTimes(1)
  })

  it('keeps a draft typed while the save was in flight', async () => {
    const request = deferred<{ data: { configured: boolean } }>()
    mocks.saveIdentityUserProviderCredential.mockReturnValue(request.promise)
    const { result } = renderCredentials()

    act(() => result.current.updateToken('user_1', 'copilot', 'submitted-token'))
    act(() => result.current.save('copilot'))
    act(() => result.current.updateToken('user_1', 'copilot', 'newer-draft'))
    await act(async () => request.resolve({ data: { configured: true } }))

    await waitFor(() => expect(mocks.toast.success).toHaveBeenCalledWith('Provider token saved'))
    expect(result.current.tokens).toEqual({ copilot: 'newer-draft' })
  })

  it('removes the stored token and clears the draft it started with', async () => {
    mocks.removeIdentityUserProviderCredential.mockResolvedValue({ data: { status: 'deleted' } })
    const { result } = renderCredentials()

    act(() => result.current.updateToken('user_1', 'copilot', 'unsaved-draft'))
    await act(async () => result.current.remove('copilot'))

    expect(mocks.removeIdentityUserProviderCredential).toHaveBeenCalledWith({
      data: { userId: 'user_1', providerKey: 'copilot' },
    })
    await waitFor(() => expect(result.current.tokens).toEqual({ copilot: '' }))
    expect(mocks.toast.success).toHaveBeenCalledWith('Provider token removed')
  })
})

describe('useOnboardingReset', () => {
  function renderOnboardingReset(user: UserView, updateForm: UpdateUserInput) {
    return renderHook(() => {
      const transition = useUsersTransition()
      return useOnboardingReset({ user, updateForm, oidcProviders, oauthProviders, transition })
    })
  }

  const switchToGithub = (user: UserView): UpdateUserInput => ({
    ...updateFormFor(user),
    auth_mode: 'oauth',
    oauth_provider_key: 'github',
  })

  it('saves an invited user’s new auth method before reissuing the URL', async () => {
    const user = invitedUser()
    const githubSignIn = {
      kind: 'oauth_sign_in' as const,
      provider_label: 'GitHub',
      sign_in_url: 'http://example.test/api/v1/auth/oauth/start?provider=github',
      user,
    }
    mocks.updateIdentityUser.mockResolvedValue({ data: user })
    mocks.resetIdentityUserOnboarding.mockResolvedValue({ data: githubSignIn })
    const { result } = renderOnboardingReset(user, switchToGithub(user))

    await act(async () => result.current.resetOnboarding())

    await waitFor(() => expect(result.current.onboardingResult).toEqual(githubSignIn))
    expect(mocks.updateIdentityUser).toHaveBeenCalledWith({
      data: {
        userId: 'user_1',
        input: {
          global_role: 'user',
          team_id: null,
          team_role: null,
          auth_mode: 'oauth',
          oidc_provider_key: null,
          oauth_provider_key: 'github',
        },
      },
    })
    expect(mocks.updateIdentityUser.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.resetIdentityUserOnboarding.mock.invocationCallOrder[0],
    )
    expect(mocks.toast.success).toHaveBeenCalledWith('SSO sign-in URL regenerated')
    expect(mocks.router.invalidate).toHaveBeenCalledTimes(1)
  })

  it('reissues without saving when the auth method is unchanged or the user is not invited', async () => {
    mocks.resetIdentityUserOnboarding.mockResolvedValue({ data: passwordInvite })
    const unchanged = renderOnboardingReset(invitedUser(), updateFormFor(invitedUser()))
    const disabledUser = invitedUser({ status: 'disabled' })
    const disabled = renderOnboardingReset(disabledUser, switchToGithub(disabledUser))

    await act(async () => unchanged.result.current.resetOnboarding())
    await act(async () => disabled.result.current.resetOnboarding())

    await waitFor(() => expect(mocks.resetIdentityUserOnboarding).toHaveBeenCalledTimes(2))
    expect(mocks.updateIdentityUser).not.toHaveBeenCalled()
    expect(mocks.toast.success).toHaveBeenCalledWith('Password invite regenerated')
  })

  it('says the auth method was saved when only the reset fails', async () => {
    const user = invitedUser()
    mocks.updateIdentityUser.mockResolvedValue({ data: user })
    mocks.resetIdentityUserOnboarding.mockRejectedValue(new Error('rate limited'))
    const { result } = renderOnboardingReset(user, switchToGithub(user))

    await act(async () => result.current.resetOnboarding())

    await waitFor(() =>
      expect(mocks.toast.error).toHaveBeenCalledWith(
        'Auth method was saved, but reset failed: rate limited',
      ),
    )
    expect(mocks.router.invalidate).toHaveBeenCalledTimes(1)
    expect(result.current.onboardingResult).toBeNull()
  })

  it('reports a reset failure on its own when nothing was saved', async () => {
    mocks.resetIdentityUserOnboarding.mockRejectedValue(new Error('rate limited'))
    const { result } = renderOnboardingReset(invitedUser(), updateFormFor(invitedUser()))

    await act(async () => result.current.resetOnboarding())

    await waitFor(() => expect(mocks.toast.error).toHaveBeenCalledWith('rate limited'))
    expect(mocks.router.invalidate).not.toHaveBeenCalled()
  })
})

describe('useUserDetails', () => {
  function renderUserDetails(initialUser: UserView | null) {
    return renderHook(
      ({ user }: { user: UserView | null }) => {
        const transition = useUsersTransition()
        return useUserDetails({ user, oidcProviders, oauthProviders, transition })
      },
      { initialProps: { user: initialUser } },
    )
  }

  it('starts from a blank form when no user is selected', () => {
    const { result } = renderUserDetails(null)

    expect(result.current.updateForm).toEqual(initialUpdateUserForm)
  })

  it('loads the form from the user and leaves an owner’s team role out of it', () => {
    const owner = invitedUser({
      auth_mode: 'oidc',
      team_id: 'team_1',
      team_name: 'Platform',
      team_role: 'owner',
      tags: [{ key: 'cost-center', value: 'platform' }],
      onboarding: {
        kind: 'oidc_sign_in',
        provider_key: 'okta',
        provider_label: 'Okta',
        sign_in_url: 'http://example.test/api/v1/auth/oidc/start?provider=okta',
      },
    })
    const { result } = renderUserDetails(owner)

    expect(result.current.updateForm).toEqual({
      global_role: 'user',
      team_id: 'team_1',
      team_role: null,
      auth_mode: 'oidc',
      oidc_provider_key: 'okta',
      oauth_provider_key: null,
      tags: [{ key: 'cost-center', value: 'platform' }],
    })
  })

  it('keeps edits until the user record refreshes, then shows the saved values', () => {
    const user = invitedUser()
    const { result, rerender } = renderUserDetails(user)

    act(() =>
      result.current.setUpdateForm((current) => ({ ...current, global_role: 'platform_admin' })),
    )
    rerender({ user })
    expect(result.current.updateForm.global_role).toBe('platform_admin')

    rerender({ user: { ...user } })
    expect(result.current.updateForm.global_role).toBe('user')
  })

  it('switches auth mode with the same provider rules as the create form', () => {
    const { result } = renderUserDetails(invitedUser())

    act(() => result.current.setUpdateAuthMode('oidc'))

    expect(result.current.updateForm).toMatchObject({
      auth_mode: 'oidc',
      oidc_provider_key: 'okta',
      oauth_provider_key: null,
    })
  })

  it('saves the sanitized form, then refreshes the list and closes the dialog', async () => {
    mocks.updateIdentityUser.mockResolvedValue({ data: invitedUser() })
    const { result } = renderUserDetails(invitedUser())
    const { event, preventDefault } = submitEvent()

    act(() => result.current.setUpdateForm((current) => ({ ...current, team_id: 'team_1' })))
    await act(async () => result.current.saveUser(event))

    expect(preventDefault).toHaveBeenCalled()
    expect(mocks.updateIdentityUser).toHaveBeenCalledWith({
      data: {
        userId: 'user_1',
        input: {
          global_role: 'user',
          tags: [],
          team_id: 'team_1',
          team_role: 'member',
          auth_mode: 'password',
          oidc_provider_key: null,
          oauth_provider_key: null,
        },
      },
    })
    await waitFor(() =>
      expect(mocks.router.navigate).toHaveBeenCalledWith({ to: '/identity/users', search: {} }),
    )
    expect(mocks.toast.success).toHaveBeenCalledWith('User updated')
    expect(mocks.router.invalidate).toHaveBeenCalledTimes(1)
  })

  it('keeps the dialog open when saving fails', async () => {
    mocks.updateIdentityUser.mockRejectedValue(new Error('Team is full'))
    const { result } = renderUserDetails(invitedUser())

    await act(async () => result.current.saveUser(submitEvent().event))

    await waitFor(() => expect(mocks.toast.error).toHaveBeenCalledWith('Team is full'))
    expect(mocks.router.navigate).not.toHaveBeenCalled()
  })

  it('deactivates and reactivates through their own endpoints', async () => {
    mocks.deactivateIdentityUser.mockResolvedValue({ data: {} })
    mocks.reactivateIdentityUser.mockResolvedValue({ data: {} })
    const { result } = renderUserDetails(invitedUser({ status: 'active' }))

    await act(async () => result.current.deactivateUser())
    await waitFor(() => expect(mocks.toast.success).toHaveBeenCalledWith('User deactivated'))
    await act(async () => result.current.reactivateUser())
    await waitFor(() => expect(mocks.toast.success).toHaveBeenCalledWith('User reactivated'))

    expect(mocks.deactivateIdentityUser).toHaveBeenCalledWith({ data: { userId: 'user_1' } })
    expect(mocks.reactivateIdentityUser).toHaveBeenCalledWith({ data: { userId: 'user_1' } })
    expect(mocks.router.navigate).toHaveBeenCalledTimes(2)
  })

  it('navigates between sections for the selected user', () => {
    const { result } = renderUserDetails(invitedUser())

    act(() => result.current.showSection('auth'))

    expect(mocks.router.navigate).toHaveBeenCalledWith({
      to: '/identity/users',
      search: { user_id: 'user_1', user_section: 'auth' },
    })
  })

  it('drops provider token drafts and edits when closed', () => {
    const { result } = renderUserDetails(invitedUser())

    act(() => result.current.credentials.updateToken('user_1', 'copilot', 'unsaved-draft'))
    act(() =>
      result.current.setUpdateForm((current) => ({ ...current, global_role: 'platform_admin' })),
    )
    act(() => result.current.close())

    expect(mocks.router.navigate).toHaveBeenCalledWith({ to: '/identity/users', search: {} })
    expect(result.current.credentials.tokens).toEqual({})
    expect(result.current.updateForm).toEqual(initialUpdateUserForm)
  })
})
