import { useState, useTransition, type FormEvent } from 'react'
import { UserIcon } from '@hugeicons/core-free-icons'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { requireAdminSession } from '@/routes/-admin-guard'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  deactivateIdentityUser,
  createIdentityUser,
  getUsers,
  reactivateIdentityUser,
  resetIdentityUserOnboarding,
  resendIdentityUserPasswordInvite,
  updateIdentityUser,
} from '@/server/admin-data.functions'
import type {
  CreateUserInput,
  CreateUserResult,
  IdentityUsersPayload,
  UpdateUserInput,
  UserView,
} from '@/types/api'

export const Route = createFileRoute('/identity/users')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: () => getUsers(),
  component: UsersPage,
})

const initialForm: CreateUserInput = {
  name: '',
  email: '',
  auth_mode: 'password',
  global_role: 'user',
  team_id: null,
  team_role: null,
  oidc_provider_key: null,
}

const initialUpdateForm: UpdateUserInput = {
  global_role: 'user',
  team_id: null,
  team_role: null,
  auth_mode: 'password',
  oidc_provider_key: null,
}

type UserDialogState = { mode: 'closed' } | { mode: 'edit'; userId: string }

export function UsersPage() {
  const router = useRouter()
  const {
    data: { users, teams, oidc_providers: oidcProviders },
  } = Route.useLoaderData() as { data: IdentityUsersPayload }
  const [isOpen, setIsOpen] = useState(false)
  const [form, setForm] = useState<CreateUserInput>(initialForm)
  const [result, setResult] = useState<CreateUserResult | null>(null)
  const [userDialog, setUserDialog] = useState<UserDialogState>({ mode: 'closed' })
  const [updateForm, setUpdateForm] = useState<UpdateUserInput>(initialUpdateForm)
  const [onboardingResult, setOnboardingResult] = useState<CreateUserResult | null>(null)
  const [isPending, startTransition] = useTransition()
  const selectedUser =
    userDialog.mode === 'edit'
      ? (users.find((user) => user.id === userDialog.userId) ?? null)
      : null

  function resetDialog() {
    setForm(initialForm)
    setResult(null)
    setIsOpen(false)
  }

  function resetUserDialog() {
    setUserDialog({ mode: 'closed' })
    setUpdateForm(initialUpdateForm)
    setOnboardingResult(null)
  }

  function setAuthMode(authMode: CreateUserInput['auth_mode']) {
    setForm((current) => ({
      ...current,
      auth_mode: authMode,
      oidc_provider_key:
        authMode === 'oidc'
          ? (current.oidc_provider_key ??
            (oidcProviders.length === 1 ? oidcProviders[0].key : null))
          : null,
    }))
  }

  function setUpdateAuthMode(authMode: UpdateUserInput['auth_mode']) {
    setUpdateForm((current) => ({
      ...current,
      auth_mode: authMode,
      oidc_provider_key:
        authMode === 'oidc'
          ? (current.oidc_provider_key ??
            (oidcProviders.length === 1 ? oidcProviders[0].key : null))
          : null,
    }))
  }

  function openUserDialog(user: UserView) {
    setUserDialog({ mode: 'edit', userId: user.id })
    setUpdateForm({
      global_role: user.global_role,
      team_id: user.team_id,
      team_role: user.team_role === 'owner' ? null : user.team_role,
      auth_mode: user.auth_mode,
      oidc_provider_key:
        user.onboarding?.kind === 'oidc_sign_in' ? user.onboarding.provider_key : null,
    })
    setOnboardingResult(null)
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()

    startTransition(async () => {
      try {
        const response = await createIdentityUser({ data: sanitizeForm(form) })
        setResult(response.data)
        toast.success(
          response.data.kind === 'password_invite'
            ? 'Password invite created'
            : 'SSO sign-in URL created',
        )
        await refreshUsers()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function refreshUsers() {
    await router.invalidate()
  }

  async function handleUpdateUser(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (userDialog.mode !== 'edit' || !selectedUser) {
      return
    }

    startTransition(async () => {
      try {
        await updateIdentityUser({
          data: {
            userId: selectedUser.id,
            input: sanitizeUpdateForm(updateForm, selectedUser, oidcProviders),
          },
        })
        toast.success('User updated')
        await refreshUsers()
        resetUserDialog()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleDeactivateUser() {
    if (userDialog.mode !== 'edit' || !selectedUser) {
      return
    }

    startTransition(async () => {
      try {
        await deactivateIdentityUser({ data: { userId: selectedUser.id } })
        toast.success('User deactivated')
        await refreshUsers()
        resetUserDialog()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleReactivateUser() {
    if (userDialog.mode !== 'edit' || !selectedUser) {
      return
    }

    startTransition(async () => {
      try {
        await reactivateIdentityUser({ data: { userId: selectedUser.id } })
        toast.success('User reactivated')
        await refreshUsers()
        resetUserDialog()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleResetUserOnboarding() {
    if (userDialog.mode !== 'edit' || !selectedUser) {
      return
    }

    startTransition(async () => {
      try {
        const response = await resetIdentityUserOnboarding({ data: { userId: selectedUser.id } })
        setOnboardingResult(response.data)
        toast.success(
          response.data.kind === 'password_invite'
            ? 'Password invite regenerated'
            : 'SSO sign-in URL regenerated',
        )
        await refreshUsers()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleCopy(value: string, message: string) {
    try {
      await navigator.clipboard.writeText(value)
      toast.success(message)
    } catch {
      toast.error('Clipboard access failed')
    }
  }

  function handleResend(user: UserView) {
    startTransition(async () => {
      try {
        const response = await resendIdentityUserPasswordInvite({ data: { userId: user.id } })
        await handleCopy(response.data.invite_url, 'Invite URL copied')
        await refreshUsers()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  function renderOnboardingActions(user: UserView) {
    if (user.onboarding?.kind === 'password_invite') {
      return (
        <>
          {user.onboarding.invite_url ? (
            <Button
              type="button"
              size="sm"
              variant="secondary"
              onClick={() => handleCopy(user.onboarding?.invite_url ?? '', 'Invite URL copied')}
            >
              Copy invite
            </Button>
          ) : null}
          <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={() => handleResend(user)}
            disabled={isPending}
          >
            Resend invite
          </Button>
        </>
      )
    }

    if (user.onboarding?.kind === 'oidc_sign_in') {
      return (
        <Button
          type="button"
          size="sm"
          variant="secondary"
          onClick={() => handleCopy(user.onboarding?.sign_in_url ?? '', 'Sign-in URL copied')}
        >
          Copy sign-in URL
        </Button>
      )
    }

    return <span className="text-xs text-[var(--color-text-soft)]">No action available</span>
  }

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-4">
          <div className="flex flex-col gap-1">
            <CardTitle>Users</CardTitle>
            <CardDescription>
              Create password or SSO users, then hand off the generated onboarding URL. A valid
              email address is also required for budget alert emails.
            </CardDescription>
          </div>

          <Dialog
            open={isOpen}
            onOpenChange={(open) => {
              setIsOpen(open)
              if (!open) {
                setResult(null)
              }
            }}
          >
            <DialogTrigger asChild>
              <Button type="button">Add user</Button>
            </DialogTrigger>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>Add user</DialogTitle>
                <DialogDescription>
                  Pre-provision the account and generate the onboarding URL to share.
                </DialogDescription>
              </DialogHeader>

              {result ? (
                <div className="flex flex-col gap-4">
                  <Alert>
                    <AlertTitle>
                      {result.kind === 'password_invite'
                        ? 'Password invite ready'
                        : 'SSO sign-in URL ready'}
                    </AlertTitle>
                    <AlertDescription>
                      {result.kind === 'password_invite'
                        ? `Share this invite before ${result.expires_at}.`
                        : `Share this URL with ${result.user.email} so they can finish SSO onboarding.`}
                    </AlertDescription>
                  </Alert>

                  <FieldGroup>
                    <Field>
                      <FieldLabel htmlFor="generated-url">Generated URL</FieldLabel>
                      <InputGroup>
                        <InputGroupInput
                          id="generated-url"
                          readOnly
                          value={
                            result.kind === 'password_invite'
                              ? result.invite_url
                              : result.sign_in_url
                          }
                        />
                        <InputGroupAddon align="inline-end">
                          <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            onClick={() =>
                              handleCopy(
                                result.kind === 'password_invite'
                                  ? result.invite_url
                                  : result.sign_in_url,
                                'URL copied',
                              )
                            }
                          >
                            Copy
                          </Button>
                        </InputGroupAddon>
                      </InputGroup>
                    </Field>
                  </FieldGroup>

                  <DialogFooter>
                    <Button type="button" variant="secondary" onClick={resetDialog}>
                      Close
                    </Button>
                  </DialogFooter>
                </div>
              ) : (
                <form className="flex flex-col gap-6" onSubmit={handleSubmit}>
                  <FieldGroup>
                    <Field>
                      <FieldLabel htmlFor="name">Name</FieldLabel>
                      <Input
                        id="name"
                        value={form.name}
                        onChange={(event) =>
                          setForm((current) => ({ ...current, name: event.target.value }))
                        }
                        placeholder="Jane Operator"
                        required
                      />
                    </Field>

                    <Field>
                      <FieldLabel htmlFor="email">Email</FieldLabel>
                      <Input
                        id="email"
                        type="email"
                        value={form.email}
                        onChange={(event) =>
                          setForm((current) => ({ ...current, email: event.target.value }))
                        }
                        placeholder="jane@example.com"
                        required
                      />
                      <FieldDescription>
                        Budget threshold alerts use this email in the initial rollout, so it must be
                        valid and monitored.
                      </FieldDescription>
                    </Field>

                    <Field>
                      <FieldLabel htmlFor="auth-mode">Auth method</FieldLabel>
                      <Select value={form.auth_mode} onValueChange={setAuthMode}>
                        <SelectTrigger id="auth-mode">
                          <SelectValue placeholder="Select auth method" />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectGroup>
                            <SelectItem value="password">Password</SelectItem>
                            <SelectItem value="oidc">SSO (OIDC)</SelectItem>
                          </SelectGroup>
                        </SelectContent>
                      </Select>
                    </Field>

                    {form.auth_mode === 'oidc' ? (
                      <>
                        {oidcProviders.length === 0 ? (
                          <Alert>
                            <AlertTitle>No SSO providers configured</AlertTitle>
                            <AlertDescription>
                              Add an OIDC provider in the gateway before inviting users with SSO, or
                              use password onboarding for now.
                            </AlertDescription>
                          </Alert>
                        ) : null}

                        <Field>
                          <FieldLabel htmlFor="oidc-provider">OIDC provider</FieldLabel>
                          <Select
                            value={form.oidc_provider_key ?? undefined}
                            onValueChange={(value) =>
                              setForm((current) => ({ ...current, oidc_provider_key: value }))
                            }
                          >
                            <SelectTrigger id="oidc-provider">
                              <SelectValue placeholder="Select provider" />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectGroup>
                                {oidcProviders.map((provider) => (
                                  <SelectItem key={provider.key} value={provider.key}>
                                    {provider.label}
                                  </SelectItem>
                                ))}
                              </SelectGroup>
                            </SelectContent>
                          </Select>
                          <FieldDescription>
                            Activation happens after a successful redirect back from this provider.
                          </FieldDescription>
                        </Field>
                      </>
                    ) : null}

                    <Field>
                      <FieldLabel htmlFor="global-role">Global role</FieldLabel>
                      <Select
                        value={form.global_role}
                        onValueChange={(value: CreateUserInput['global_role']) =>
                          setForm((current) => ({ ...current, global_role: value }))
                        }
                      >
                        <SelectTrigger id="global-role">
                          <SelectValue placeholder="Select role" />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectGroup>
                            <SelectItem value="user">User</SelectItem>
                            <SelectItem value="platform_admin">Platform admin</SelectItem>
                          </SelectGroup>
                        </SelectContent>
                      </Select>
                    </Field>

                    <Field>
                      <FieldLabel htmlFor="team">Team</FieldLabel>
                      <Select
                        value={form.team_id ?? 'none'}
                        onValueChange={(value) =>
                          setForm((current) => ({
                            ...current,
                            team_id: value === 'none' ? null : value,
                            team_role: value === 'none' ? null : (current.team_role ?? 'member'),
                          }))
                        }
                      >
                        <SelectTrigger id="team">
                          <SelectValue placeholder="No team" />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectGroup>
                            <SelectItem value="none">No team</SelectItem>
                            {teams.map((team) => (
                              <SelectItem key={team.id} value={team.id}>
                                {team.name}
                              </SelectItem>
                            ))}
                          </SelectGroup>
                        </SelectContent>
                      </Select>
                    </Field>

                    {form.team_id ? (
                      <Field>
                        <FieldLabel htmlFor="team-role">Team role</FieldLabel>
                        <Select
                          value={form.team_role ?? 'member'}
                          onValueChange={(value: NonNullable<CreateUserInput['team_role']>) =>
                            setForm((current) => ({ ...current, team_role: value }))
                          }
                        >
                          <SelectTrigger id="team-role">
                            <SelectValue placeholder="Select team role" />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectGroup>
                              <SelectItem value="member">Member</SelectItem>
                              <SelectItem value="admin">Admin</SelectItem>
                            </SelectGroup>
                          </SelectContent>
                        </Select>
                      </Field>
                    ) : null}
                  </FieldGroup>

                  <DialogFooter>
                    <Button type="button" variant="secondary" onClick={resetDialog}>
                      Cancel
                    </Button>
                    <Button
                      type="submit"
                      disabled={isPending || isOidcDisabled(form, oidcProviders)}
                    >
                      {isPending ? 'Creating…' : 'Create user'}
                    </Button>
                  </DialogFooter>
                </form>
              )}
            </DialogContent>
          </Dialog>
        </CardHeader>

        <CardContent>
          {users.length === 0 ? (
            <Empty>
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <AppIcon icon={UserIcon} size={22} stroke={1.5} />
                </EmptyMedia>
                <EmptyTitle>No users provisioned yet</EmptyTitle>
                <EmptyDescription>
                  Create the first platform or team user, then share the generated onboarding URL
                  directly from the dialog.
                </EmptyDescription>
              </EmptyHeader>
              <EmptyContent>
                <Button type="button" onClick={() => setIsOpen(true)}>
                  Create first user
                </Button>
              </EmptyContent>
            </Empty>
          ) : (
            <div className="flex flex-col gap-4">
              <div className="grid gap-3 md:hidden">
                {users.map((user) => (
                  <article
                    key={user.id}
                    className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4"
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <p className="truncate font-semibold text-[var(--color-text)]">
                          {user.name}
                        </p>
                        <p className="truncate text-sm text-[var(--color-text-muted)]">
                          {user.email}
                        </p>
                      </div>
                      <Badge
                        variant={
                          user.status === 'active'
                            ? 'success'
                            : user.status === 'invited'
                              ? 'warning'
                              : 'default'
                        }
                      >
                        {user.status}
                      </Badge>
                    </div>

                    <dl className="mt-4 grid grid-cols-2 gap-x-4 gap-y-3 text-sm">
                      <div>
                        <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                          Auth
                        </dt>
                        <dd className="text-[var(--color-text-muted)]">{user.auth_mode}</dd>
                      </div>
                      <div>
                        <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                          Global role
                        </dt>
                        <dd className="text-[var(--color-text-muted)]">{user.global_role}</dd>
                      </div>
                      <div>
                        <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                          Team
                        </dt>
                        <dd className="text-[var(--color-text-muted)]">
                          {user.team_name ?? 'No team'}
                        </dd>
                      </div>
                      <div>
                        <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                          Team role
                        </dt>
                        <dd className="text-[var(--color-text-muted)]">{user.team_role ?? '—'}</dd>
                      </div>
                      <div>
                        <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                          Logs
                        </dt>
                        <dd className="text-[var(--color-text-muted)]">
                          {user.request_logging_enabled ? 'Enabled' : 'Disabled'}
                        </dd>
                      </div>
                    </dl>

                    <div className="mt-4 flex flex-wrap gap-2">
                      <Button
                        type="button"
                        size="sm"
                        variant="secondary"
                        onClick={() => openUserDialog(user)}
                      >
                        Manage
                      </Button>
                      {renderOnboardingActions(user)}
                    </div>
                  </article>
                ))}
              </div>

              <div className="hidden overflow-hidden rounded-md border border-[color:var(--color-border)] md:block">
                <table className="w-full text-left text-sm">
                  <thead className="bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]">
                    <tr>
                      <th className="px-3 py-2 font-semibold">Name</th>
                      <th className="px-3 py-2 font-semibold">Email</th>
                      <th className="px-3 py-2 font-semibold">Auth</th>
                      <th className="px-3 py-2 font-semibold">Global role</th>
                      <th className="px-3 py-2 font-semibold">Logs</th>
                      <th className="px-3 py-2 font-semibold">Team</th>
                      <th className="px-3 py-2 font-semibold">Team role</th>
                      <th className="px-3 py-2 font-semibold">Status</th>
                      <th className="px-3 py-2 font-semibold">Onboarding</th>
                      <th className="px-3 py-2 font-semibold">Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {users.map((user) => (
                      <tr
                        key={user.id}
                        className="border-t border-[color:var(--color-border)] align-top"
                      >
                        <td className="px-3 py-3 text-[var(--color-text)]">{user.name}</td>
                        <td className="px-3 py-3 text-[var(--color-text-muted)]">{user.email}</td>
                        <td className="px-3 py-3 text-[var(--color-text-muted)]">
                          {user.auth_mode}
                        </td>
                        <td className="px-3 py-3 text-[var(--color-text-muted)]">
                          {user.global_role}
                        </td>
                        <td className="px-3 py-3 text-[var(--color-text-muted)]">
                          {user.request_logging_enabled ? 'Enabled' : 'Disabled'}
                        </td>
                        <td className="px-3 py-3 text-[var(--color-text-muted)]">
                          {user.team_name ?? '—'}
                        </td>
                        <td className="px-3 py-3 text-[var(--color-text-muted)]">
                          {user.team_role ?? '—'}
                        </td>
                        <td className="px-3 py-3">
                          <Badge
                            variant={
                              user.status === 'active'
                                ? 'success'
                                : user.status === 'invited'
                                  ? 'warning'
                                  : 'default'
                            }
                          >
                            {user.status}
                          </Badge>
                        </td>
                        <td className="px-3 py-3">
                          <div className="flex flex-wrap gap-2">
                            {renderOnboardingActions(user)}
                          </div>
                        </td>
                        <td className="px-3 py-3">
                          <Button
                            type="button"
                            size="sm"
                            variant="secondary"
                            onClick={() => openUserDialog(user)}
                          >
                            Manage
                          </Button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      <Dialog
        open={userDialog.mode === 'edit'}
        onOpenChange={(open) => {
          if (!open) {
            resetUserDialog()
          }
        }}
      >
        <DialogContent className="w-[min(760px,calc(100vw-32px))]">
          <DialogHeader>
            <DialogTitle>Manage user</DialogTitle>
            <DialogDescription>
              Update role and membership fields, then use the lifecycle actions to deactivate,
              reactivate, or reset onboarding.
            </DialogDescription>
          </DialogHeader>

          {selectedUser ? (
            <div className="flex flex-col gap-5">
              <div className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4">
                <p className="font-semibold text-[var(--color-text)]">{selectedUser.name}</p>
                <p className="text-sm text-[var(--color-text-muted)]">{selectedUser.email}</p>
                <p className="mt-1 text-sm text-[var(--color-text-muted)]">
                  Request logging: {selectedUser.request_logging_enabled ? 'enabled' : 'disabled'}
                </p>
                <p className="mt-2 text-xs text-[var(--color-text-soft)]">
                  {selectedUser.status === 'invited'
                    ? 'Auth mode can only be changed while the user is still invited.'
                    : 'Auth mode is locked after activation; use reset onboarding to reissue credentials.'}
                </p>
                {selectedUser.team_role === 'owner' ? (
                  <Alert className="mt-3">
                    <AlertTitle>Owner membership is locked</AlertTitle>
                    <AlertDescription>
                      This user is an owner on their current team. In this slice, owner memberships
                      cannot be moved or changed through the admin UI.
                    </AlertDescription>
                  </Alert>
                ) : null}
              </div>

              <form className="flex flex-col gap-5" onSubmit={handleUpdateUser}>
                <FieldGroup>
                  <Field>
                    <FieldLabel htmlFor="manage-global-role">Global role</FieldLabel>
                    <Select
                      value={updateForm.global_role}
                      onValueChange={(value: UpdateUserInput['global_role']) =>
                        setUpdateForm((current) => ({ ...current, global_role: value }))
                      }
                    >
                      <SelectTrigger id="manage-global-role">
                        <SelectValue placeholder="Select role" />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectGroup>
                          <SelectItem value="user">User</SelectItem>
                          <SelectItem value="platform_admin">Platform admin</SelectItem>
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                  </Field>

                  <Field>
                    <FieldLabel htmlFor="manage-team">Team</FieldLabel>
                    <Select
                      value={updateForm.team_id ?? 'none'}
                      onValueChange={(value) =>
                        setUpdateForm((current) => ({
                          ...current,
                          team_id: value === 'none' ? null : value,
                          team_role: value === 'none' ? null : (current.team_role ?? 'member'),
                        }))
                      }
                      disabled={selectedUser.team_role === 'owner'}
                    >
                      <SelectTrigger id="manage-team">
                        <SelectValue placeholder="No team" />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectGroup>
                          <SelectItem value="none">No team</SelectItem>
                          {teams.map((team) => (
                            <SelectItem key={team.id} value={team.id}>
                              {team.name}
                            </SelectItem>
                          ))}
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                  </Field>

                  {updateForm.team_id ? (
                    <Field>
                      <FieldLabel htmlFor="manage-team-role">Team role</FieldLabel>
                      <Select
                        value={updateForm.team_role ?? 'member'}
                        onValueChange={(value: NonNullable<UpdateUserInput['team_role']>) =>
                          setUpdateForm((current) => ({ ...current, team_role: value }))
                        }
                        disabled={selectedUser.team_role === 'owner'}
                      >
                        <SelectTrigger id="manage-team-role">
                          <SelectValue placeholder="Select team role" />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectGroup>
                            <SelectItem value="member">Member</SelectItem>
                            <SelectItem value="admin">Admin</SelectItem>
                          </SelectGroup>
                        </SelectContent>
                      </Select>
                    </Field>
                  ) : null}

                  <Field>
                    <FieldLabel htmlFor="manage-auth-mode">Auth method</FieldLabel>
                    <Select
                      value={updateForm.auth_mode}
                      onValueChange={setUpdateAuthMode}
                      disabled={selectedUser.status !== 'invited'}
                    >
                      <SelectTrigger id="manage-auth-mode">
                        <SelectValue placeholder="Select auth method" />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectGroup>
                          <SelectItem value="password">Password</SelectItem>
                          <SelectItem value="oidc">SSO (OIDC)</SelectItem>
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                    <FieldDescription>
                      {selectedUser.status === 'invited'
                        ? 'You can switch onboarding mode before the user completes setup.'
                        : 'Auth mode is read-only after activation.'}
                    </FieldDescription>
                  </Field>

                  {updateForm.auth_mode === 'oidc' ? (
                    <>
                      {oidcProviders.length === 0 ? (
                        <Alert>
                          <AlertTitle>No SSO providers configured</AlertTitle>
                          <AlertDescription>
                            Add an OIDC provider in the gateway before switching a user to SSO.
                          </AlertDescription>
                        </Alert>
                      ) : null}

                      <Field>
                        <FieldLabel htmlFor="manage-oidc-provider">OIDC provider</FieldLabel>
                        <Select
                          value={updateForm.oidc_provider_key ?? undefined}
                          onValueChange={(value) =>
                            setUpdateForm((current) => ({
                              ...current,
                              oidc_provider_key: value,
                            }))
                          }
                          disabled={selectedUser.status !== 'invited'}
                        >
                          <SelectTrigger id="manage-oidc-provider">
                            <SelectValue placeholder="Select provider" />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectGroup>
                              {oidcProviders.map((provider) => (
                                <SelectItem key={provider.key} value={provider.key}>
                                  {provider.label}
                                </SelectItem>
                              ))}
                            </SelectGroup>
                          </SelectContent>
                        </Select>
                      </Field>
                    </>
                  ) : null}
                </FieldGroup>

                <section className="flex flex-col gap-3 rounded-lg border border-[color:var(--color-border)] p-4">
                  <div className="flex flex-col gap-1">
                    <h3 className="text-sm font-semibold text-[var(--color-text)]">
                      Lifecycle actions
                    </h3>
                    <p className="text-sm text-[var(--color-text-muted)]">
                      These operations take effect immediately and the list will refresh from the
                      gateway after each action.
                    </p>
                  </div>

                  <div className="flex flex-wrap gap-2">
                    {selectedUser.status !== 'disabled' ? (
                      <Button
                        type="button"
                        variant="destructive"
                        onClick={handleDeactivateUser}
                        disabled={isPending}
                      >
                        Deactivate
                      </Button>
                    ) : (
                      <Button
                        type="button"
                        variant="secondary"
                        onClick={handleReactivateUser}
                        disabled={isPending}
                      >
                        Reactivate
                      </Button>
                    )}

                    <Button
                      type="button"
                      variant="ghost"
                      onClick={handleResetUserOnboarding}
                      disabled={
                        isPending ||
                        (selectedUser.status !== 'invited' && selectedUser.status !== 'disabled')
                      }
                    >
                      Reset onboarding
                    </Button>
                  </div>

                  {onboardingResult ? (
                    <Alert>
                      <AlertTitle>
                        {onboardingResult.kind === 'password_invite'
                          ? 'Password invite ready'
                          : 'SSO sign-in URL ready'}
                      </AlertTitle>
                      <AlertDescription>
                        {onboardingResult.kind === 'password_invite'
                          ? `Share this invite before ${onboardingResult.expires_at}.`
                          : `Share this URL with ${onboardingResult.user.email} so they can finish SSO onboarding.`}
                      </AlertDescription>

                      <Field className="mt-3">
                        <FieldLabel htmlFor="reset-onboarding-url">Generated URL</FieldLabel>
                        <InputGroup>
                          <InputGroupInput
                            id="reset-onboarding-url"
                            readOnly
                            value={
                              onboardingResult.kind === 'password_invite'
                                ? onboardingResult.invite_url
                                : onboardingResult.sign_in_url
                            }
                          />
                          <InputGroupAddon align="inline-end">
                            <Button
                              type="button"
                              variant="ghost"
                              size="sm"
                              onClick={() =>
                                handleCopy(
                                  onboardingResult.kind === 'password_invite'
                                    ? onboardingResult.invite_url
                                    : onboardingResult.sign_in_url,
                                  'URL copied',
                                )
                              }
                            >
                              Copy
                            </Button>
                          </InputGroupAddon>
                        </InputGroup>
                      </Field>
                    </Alert>
                  ) : null}
                </section>

                <DialogFooter>
                  <Button type="button" variant="secondary" onClick={resetUserDialog}>
                    Cancel
                  </Button>
                  <Button
                    type="submit"
                    disabled={
                      isPending ||
                      (updateForm.auth_mode === 'oidc' &&
                        (oidcProviders.length === 0 || !updateForm.oidc_provider_key))
                    }
                  >
                    {isPending ? 'Saving…' : 'Save changes'}
                  </Button>
                </DialogFooter>
              </form>
            </div>
          ) : null}
        </DialogContent>
      </Dialog>
    </div>
  )
}

function sanitizeForm(form: CreateUserInput): CreateUserInput {
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
  }
}

function sanitizeUpdateForm(
  form: UpdateUserInput,
  user: UserView,
  oidcProviders: IdentityUsersPayload['oidc_providers'],
): UpdateUserInput {
  const update: UpdateUserInput = {
    global_role: form.global_role,
  }

  if (user.team_role !== 'owner') {
    update.team_id = form.team_id ?? null
    update.team_role = form.team_id ? (form.team_role ?? 'member') : null
  }

  if (user.status === 'invited') {
    update.auth_mode = form.auth_mode
    update.oidc_provider_key = form.auth_mode === 'oidc' ? (form.oidc_provider_key ?? null) : null
  }

  if (user.status === 'invited' && update.auth_mode === 'oidc') {
    const validProvider = oidcProviders.find(
      (provider) => provider.key === update.oidc_provider_key,
    )
    update.oidc_provider_key = validProvider ? update.oidc_provider_key : null
  }

  return update
}

function isOidcDisabled(form: CreateUserInput, providers: IdentityUsersPayload['oidc_providers']) {
  return form.auth_mode === 'oidc' && (providers.length === 0 || !form.oidc_provider_key)
}

function getErrorMessage(error: unknown) {
  return error instanceof Error ? error.message : 'Something went wrong'
}
