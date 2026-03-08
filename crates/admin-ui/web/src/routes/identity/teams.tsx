import { useMemo, useState, useTransition, type FormEvent } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
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
  addIdentityTeamMembers,
  createIdentityTeam,
  createIdentityUser,
  getTeams,
  updateIdentityTeam,
} from '@/server/admin-data.functions'
import type {
  CreateTeamInput,
  CreateUserInput,
  CreateUserResult,
  IdentityTeamsPayload,
  TeamAssignableUserView,
  TeamManagementView,
} from '@/types/api'

export const Route = createFileRoute('/identity/teams')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: () => getTeams(),
  component: TeamsPage,
})

const initialTeamForm: CreateTeamInput = {
  name: '',
  admin_user_ids: [],
}

const initialInviteForm: CreateUserInput = {
  name: '',
  email: '',
  auth_mode: 'password',
  global_role: 'user',
  team_id: null,
  team_role: 'member',
  oidc_provider_key: null,
}

type TeamDialogState =
  | { mode: 'closed' }
  | { mode: 'create' }
  | { mode: 'edit'; teamId: string }

type MembersDialogState =
  | { mode: 'closed' }
  | { mode: 'open'; teamId: string }

function TeamsPage() {
  const router = useRouter()
  const {
    data: { teams, users, oidc_providers: oidcProviders },
  } = Route.useLoaderData() as { data: IdentityTeamsPayload }
  const [teamDialog, setTeamDialog] = useState<TeamDialogState>({ mode: 'closed' })
  const [teamForm, setTeamForm] = useState<CreateTeamInput>(initialTeamForm)
  const [membersDialog, setMembersDialog] = useState<MembersDialogState>({ mode: 'closed' })
  const [selectedExistingMemberIds, setSelectedExistingMemberIds] = useState<string[]>([])
  const [inviteForm, setInviteForm] = useState<CreateUserInput>(initialInviteForm)
  const [inviteResult, setInviteResult] = useState<CreateUserResult | null>(null)
  const [isPending, startTransition] = useTransition()

  const editingTeam =
    teamDialog.mode === 'edit' ? teams.find((team) => team.id === teamDialog.teamId) ?? null : null
  const membersTeam =
    membersDialog.mode === 'open'
      ? teams.find((team) => team.id === membersDialog.teamId) ?? null
      : null

  const adminOptions = useMemo(
    () =>
      users.map((user) => ({
        user,
        disabled:
          user.team_id !== null &&
          (editingTeam ? user.team_id !== editingTeam.id : true),
        reason:
          user.team_id !== null && (!editingTeam || user.team_id !== editingTeam.id)
            ? `Already in ${user.team_name ?? 'another team'}`
            : null,
      })),
    [editingTeam, users],
  )

  const existingMemberOptions = useMemo(
    () =>
      users.map((user) => ({
        user,
        disabled:
          user.team_id !== null &&
          (!membersTeam || user.team_id !== membersTeam.id),
        reason:
          user.team_id !== null && (!membersTeam || user.team_id !== membersTeam.id)
            ? `Already in ${user.team_name ?? 'another team'}`
            : user.team_id === membersTeam?.id
              ? 'Already on this team'
              : null,
      })),
    [membersTeam, users],
  )

  async function refreshTeams() {
    await router.invalidate()
  }

  function openCreateTeamDialog() {
    setTeamForm(initialTeamForm)
    setTeamDialog({ mode: 'create' })
  }

  function openEditTeamDialog(team: TeamManagementView) {
    setTeamForm({
      name: team.name,
      admin_user_ids: team.admins.map((admin) => admin.id),
    })
    setTeamDialog({ mode: 'edit', teamId: team.id })
  }

  function closeTeamDialog() {
    setTeamDialog({ mode: 'closed' })
    setTeamForm(initialTeamForm)
  }

  function openMembersDialog(team: TeamManagementView) {
    setMembersDialog({ mode: 'open', teamId: team.id })
    setSelectedExistingMemberIds([])
    setInviteResult(null)
    setInviteForm({
      ...initialInviteForm,
      team_id: team.id,
      team_role: 'member',
    })
  }

  function closeMembersDialog() {
    setMembersDialog({ mode: 'closed' })
    setSelectedExistingMemberIds([])
    setInviteResult(null)
    setInviteForm(initialInviteForm)
  }

  function setInviteAuthMode(authMode: CreateUserInput['auth_mode']) {
    setInviteForm((current) => ({
      ...current,
      auth_mode: authMode,
      oidc_provider_key:
        authMode === 'oidc'
          ? (current.oidc_provider_key ?? (oidcProviders.length === 1 ? oidcProviders[0].key : null))
          : null,
    }))
  }

  async function handleTeamSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()

    startTransition(async () => {
      try {
        if (teamDialog.mode === 'edit' && editingTeam) {
          await updateIdentityTeam({
            data: {
              teamId: editingTeam.id,
              input: sanitizeTeamForm(teamForm),
            },
          })
          toast.success('Team updated')
        } else {
          await createIdentityTeam({ data: sanitizeTeamForm(teamForm) })
          toast.success('Team created')
        }
        await refreshTeams()
        closeTeamDialog()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleAddExistingMembers(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!membersTeam || selectedExistingMemberIds.length === 0) {
      return
    }

    startTransition(async () => {
      try {
        await addIdentityTeamMembers({
          data: {
            teamId: membersTeam.id,
            input: { user_ids: selectedExistingMemberIds },
          },
        })
        toast.success('Members added')
        setSelectedExistingMemberIds([])
        await refreshTeams()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleInviteMember(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!membersTeam) {
      return
    }

    startTransition(async () => {
      try {
        const response = await createIdentityUser({
          data: sanitizeInviteForm(inviteForm, oidcProviders),
        })
        setInviteResult(response.data)
        toast.success(
          response.data.kind === 'password_invite'
            ? 'Member invite created'
            : 'Member sign-in URL created',
        )
        await refreshTeams()
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

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-4">
          <div className="flex flex-col gap-1">
            <CardTitle>Teams</CardTitle>
            <CardDescription>
              Create teams, assign team admins, and add members over time as users arrive.
            </CardDescription>
          </div>
          <Button type="button" onClick={openCreateTeamDialog}>
            Add team
          </Button>
        </CardHeader>
        <CardContent>
          <div className="overflow-hidden rounded-md border border-neutral-800">
            <table className="w-full text-left text-sm">
              <thead className="bg-neutral-900/70 text-neutral-400">
                <tr>
                  <th className="px-3 py-2 font-medium">Team</th>
                  <th className="px-3 py-2 font-medium">Admins</th>
                  <th className="px-3 py-2 font-medium">Members</th>
                  <th className="px-3 py-2 font-medium">Status</th>
                  <th className="px-3 py-2 font-medium">Actions</th>
                </tr>
              </thead>
              <tbody>
                {teams.map((team) => (
                  <tr key={team.id} className="border-t border-neutral-800 align-top">
                    <td className="px-3 py-3">
                      <div className="flex flex-col gap-1">
                        <p className="text-neutral-100">{team.name}</p>
                        <p className="text-xs text-neutral-500">{team.key}</p>
                      </div>
                    </td>
                    <td className="px-3 py-3">
                      {team.admins.length > 0 ? (
                        <div className="flex flex-wrap gap-2">
                          {team.admins.map((admin) => (
                            <Badge key={admin.id}>{admin.name}</Badge>
                          ))}
                        </div>
                      ) : (
                        <span className="text-xs text-neutral-500">No admins</span>
                      )}
                    </td>
                    <td className="px-3 py-3 text-neutral-300">{team.member_count}</td>
                    <td className="px-3 py-3">
                      <Badge variant={team.status === 'active' ? 'success' : 'warning'}>
                        {team.status}
                      </Badge>
                    </td>
                    <td className="px-3 py-3">
                      <div className="flex flex-wrap gap-2">
                        <Button
                          type="button"
                          size="sm"
                          variant="secondary"
                          onClick={() => openEditTeamDialog(team)}
                        >
                          Edit team
                        </Button>
                        <Button
                          type="button"
                          size="sm"
                          variant="ghost"
                          onClick={() => openMembersDialog(team)}
                        >
                          Add members
                        </Button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </CardContent>
      </Card>

      <Dialog open={teamDialog.mode !== 'closed'} onOpenChange={(open) => !open && closeTeamDialog()}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{teamDialog.mode === 'edit' ? 'Edit team' : 'Add team'}</DialogTitle>
            <DialogDescription>
              {teamDialog.mode === 'edit'
                ? 'Rename the team and manage which users hold team admin access.'
                : 'Create a team now and optionally assign team admins from existing users.'}
            </DialogDescription>
          </DialogHeader>

          <form className="flex flex-col gap-6" onSubmit={handleTeamSubmit}>
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="team-name">Name</FieldLabel>
                <Input
                  id="team-name"
                  value={teamForm.name}
                  onChange={(event) =>
                    setTeamForm((current) => ({ ...current, name: event.target.value }))
                  }
                  placeholder="Core Platform"
                  required
                />
              </Field>

              <UserMultiSelectField
                label="Team admins"
                description="Admins can manage the team after creation. You can leave this empty and assign them later."
                placeholder="Select team admins"
                users={adminOptions}
                selectedUserIds={teamForm.admin_user_ids}
                onChange={(adminUserIds) =>
                  setTeamForm((current) => ({ ...current, admin_user_ids: adminUserIds }))
                }
                emptyTitle="No assignable admins yet"
                emptyDescription="Create the team now and return later once users exist."
              />
            </FieldGroup>

            <DialogFooter>
              <Button type="button" variant="secondary" onClick={closeTeamDialog}>
                Cancel
              </Button>
              <Button type="submit" disabled={isPending}>
                {isPending
                  ? teamDialog.mode === 'edit'
                    ? 'Saving…'
                    : 'Creating…'
                  : teamDialog.mode === 'edit'
                    ? 'Save changes'
                    : 'Create team'}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog
        open={membersDialog.mode === 'open'}
        onOpenChange={(open) => !open && closeMembersDialog()}
      >
        <DialogContent className="w-[min(760px,calc(100vw-32px))]">
          <DialogHeader>
            <DialogTitle>Add members</DialogTitle>
            <DialogDescription>
              {membersTeam
                ? `Add existing users to ${membersTeam.name} or invite a new member directly into the team.`
                : 'Add existing users or invite a new member.'}
            </DialogDescription>
          </DialogHeader>

          {membersTeam ? (
            <div className="flex flex-col gap-6">
              <Card className="bg-neutral-950/40">
                <CardHeader>
                  <CardTitle className="text-sm">Existing users</CardTitle>
                  <CardDescription>
                    Only teamless users can be newly added. Users already on another team remain
                    unavailable in this flow.
                  </CardDescription>
                </CardHeader>
                <CardContent>
                  <form className="flex flex-col gap-4" onSubmit={handleAddExistingMembers}>
                    <UserMultiSelectField
                      label="Users"
                      description="Select existing users to add as members."
                      placeholder="Select existing users"
                      users={existingMemberOptions}
                      selectedUserIds={selectedExistingMemberIds}
                      onChange={setSelectedExistingMemberIds}
                      emptyTitle="No existing users available"
                      emptyDescription="Create or invite a new user below if the team is being set up first."
                    />

                    <div className="flex justify-end">
                      <Button
                        type="submit"
                        variant="secondary"
                        disabled={isPending || selectedExistingMemberIds.length === 0}
                      >
                        {isPending ? 'Adding…' : 'Add selected users'}
                      </Button>
                    </div>
                  </form>
                </CardContent>
              </Card>

              <Card className="bg-neutral-950/40">
                <CardHeader>
                  <CardTitle className="text-sm">Invite a new member</CardTitle>
                  <CardDescription>
                    This uses the same onboarding flow as the users page and preassigns the new
                    user to {membersTeam.name} as a member.
                  </CardDescription>
                </CardHeader>
                <CardContent className="flex flex-col gap-4">
                  {inviteResult ? (
                    <Alert>
                      <AlertTitle>
                        {inviteResult.kind === 'password_invite'
                          ? 'Member invite ready'
                          : 'Member sign-in URL ready'}
                      </AlertTitle>
                      <AlertDescription>
                        {inviteResult.kind === 'password_invite'
                          ? `Share this invite before ${inviteResult.expires_at}.`
                          : `Share this URL with ${inviteResult.user.email} so they can finish SSO onboarding.`}
                      </AlertDescription>
                    </Alert>
                  ) : null}

                  <form className="flex flex-col gap-5" onSubmit={handleInviteMember}>
                    <FieldGroup>
                      <Field>
                        <FieldLabel htmlFor="member-name">Name</FieldLabel>
                        <Input
                          id="member-name"
                          value={inviteForm.name}
                          onChange={(event) =>
                            setInviteForm((current) => ({ ...current, name: event.target.value }))
                          }
                          placeholder="Taylor Member"
                          required
                        />
                      </Field>

                      <Field>
                        <FieldLabel htmlFor="member-email">Email</FieldLabel>
                        <Input
                          id="member-email"
                          type="email"
                          value={inviteForm.email}
                          onChange={(event) =>
                            setInviteForm((current) => ({ ...current, email: event.target.value }))
                          }
                          placeholder="taylor@example.com"
                          required
                        />
                      </Field>

                      <Field>
                        <FieldLabel htmlFor="member-auth-mode">Auth method</FieldLabel>
                        <Select value={inviteForm.auth_mode} onValueChange={setInviteAuthMode}>
                          <SelectTrigger id="member-auth-mode">
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

                      {inviteForm.auth_mode === 'oidc' ? (
                        <>
                          {oidcProviders.length === 0 ? (
                            <Alert>
                              <AlertTitle>No SSO providers configured</AlertTitle>
                              <AlertDescription>
                                Add an OIDC provider in the gateway before inviting members with
                                SSO, or use password onboarding for now.
                              </AlertDescription>
                            </Alert>
                          ) : null}

                          <Field>
                            <FieldLabel htmlFor="member-oidc-provider">OIDC provider</FieldLabel>
                            <Select
                              value={inviteForm.oidc_provider_key ?? undefined}
                              onValueChange={(value) =>
                                setInviteForm((current) => ({
                                  ...current,
                                  oidc_provider_key: value,
                                }))
                              }
                            >
                              <SelectTrigger id="member-oidc-provider">
                                <SelectValue placeholder="Select provider" />
                              </SelectTrigger>
                              <SelectContent>
                                <SelectGroup>
                                  {oidcProviders.map((provider) => (
                                    <SelectItem key={provider.id} value={provider.key}>
                                      {provider.label}
                                    </SelectItem>
                                  ))}
                                </SelectGroup>
                              </SelectContent>
                            </Select>
                            <FieldDescription>
                              Activation happens after a successful redirect back from this
                              provider.
                            </FieldDescription>
                          </Field>
                        </>
                      ) : null}

                      {inviteResult ? (
                        <Field>
                          <FieldLabel htmlFor="generated-member-url">Generated URL</FieldLabel>
                          <div className="flex gap-2">
                            <Input
                              id="generated-member-url"
                              readOnly
                              value={
                                inviteResult.kind === 'password_invite'
                                  ? inviteResult.invite_url
                                  : inviteResult.sign_in_url
                              }
                            />
                            <Button
                              type="button"
                              variant="secondary"
                              onClick={() =>
                                handleCopy(
                                  inviteResult.kind === 'password_invite'
                                    ? inviteResult.invite_url
                                    : inviteResult.sign_in_url,
                                  'URL copied',
                                )
                              }
                            >
                              Copy
                            </Button>
                          </div>
                        </Field>
                      ) : null}
                    </FieldGroup>

                    <div className="flex justify-end">
                      <Button
                        type="submit"
                        disabled={isPending || isInviteOidcDisabled(inviteForm, oidcProviders)}
                      >
                        {isPending ? 'Creating…' : 'Invite member'}
                      </Button>
                    </div>
                  </form>
                </CardContent>
              </Card>

              <DialogFooter>
                <Button type="button" variant="secondary" onClick={closeMembersDialog}>
                  Close
                </Button>
              </DialogFooter>
            </div>
          ) : null}
        </DialogContent>
      </Dialog>
    </div>
  )
}

function UserMultiSelectField({
  label,
  description,
  placeholder,
  users,
  selectedUserIds,
  onChange,
  emptyTitle,
  emptyDescription,
}: {
  label: string
  description: string
  placeholder: string
  users: Array<{ user: TeamAssignableUserView; disabled: boolean; reason: string | null }>
  selectedUserIds: string[]
  onChange: (userIds: string[]) => void
  emptyTitle: string
  emptyDescription: string
}) {
  const [open, setOpen] = useState(false)
  const selectedUsers = users
    .map((entry) => entry.user)
    .filter((user) => selectedUserIds.includes(user.id))
  const selectableCount = users.filter((entry) => !entry.disabled).length

  function toggleUser(userId: string) {
    if (selectedUserIds.includes(userId)) {
      onChange(selectedUserIds.filter((value) => value !== userId))
      return
    }

    onChange([...selectedUserIds, userId])
  }

  return (
    <Field>
      <FieldLabel>{label}</FieldLabel>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            type="button"
            variant="secondary"
            className="w-full justify-between"
            disabled={users.length === 0}
          >
            <span className="truncate text-left">
              {selectedUsers.length > 0
                ? `${selectedUsers.length} selected`
                : selectableCount > 0
                  ? placeholder
                  : emptyTitle}
            </span>
            <span className="text-xs text-neutral-500">▼</span>
          </Button>
        </PopoverTrigger>
        <PopoverContent className="p-0">
          <Command>
            <CommandInput placeholder="Search users…" />
            <CommandList>
              <CommandEmpty>No matching users.</CommandEmpty>
              <CommandGroup heading={label}>
                {users.map(({ user, disabled, reason }) => (
                  <CommandItem
                    key={user.id}
                    value={`${user.name} ${user.email}`}
                    disabled={disabled}
                    onSelect={() => {
                      if (!disabled) {
                        toggleUser(user.id)
                      }
                    }}
                  >
                    <span className="w-4 text-neutral-400">
                      {selectedUserIds.includes(user.id) ? '✓' : ''}
                    </span>
                    <div className="flex min-w-0 flex-1 flex-col gap-1">
                      <span className="truncate">{user.name}</span>
                      <span className="truncate text-xs text-neutral-500">
                        {user.email}
                        {reason ? ` · ${reason}` : ''}
                      </span>
                    </div>
                  </CommandItem>
                ))}
              </CommandGroup>
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>

      {selectedUsers.length > 0 ? (
        <div className="flex flex-wrap gap-2">
          {selectedUsers.map((user) => (
            <Badge key={user.id}>{user.name}</Badge>
          ))}
        </div>
      ) : (
        <FieldDescription>{description}</FieldDescription>
      )}

      {selectableCount === 0 ? (
        <Alert>
          <AlertTitle>{emptyTitle}</AlertTitle>
          <AlertDescription>{emptyDescription}</AlertDescription>
        </Alert>
      ) : null}
    </Field>
  )
}

function sanitizeTeamForm(form: CreateTeamInput): CreateTeamInput {
  return {
    name: form.name.trim(),
    admin_user_ids: Array.from(new Set(form.admin_user_ids)),
  }
}

function sanitizeInviteForm(
  form: CreateUserInput,
  oidcProviders: IdentityTeamsPayload['oidc_providers'],
): CreateUserInput {
  return {
    name: form.name.trim(),
    email: form.email.trim(),
    auth_mode: form.auth_mode,
    global_role: 'user',
    team_id: form.team_id,
    team_role: 'member',
    oidc_provider_key:
      form.auth_mode === 'oidc'
        ? (form.oidc_provider_key ?? (oidcProviders.length === 1 ? oidcProviders[0].key : null))
        : null,
  }
}

function isInviteOidcDisabled(
  form: CreateUserInput,
  providers: IdentityTeamsPayload['oidc_providers'],
) {
  return form.auth_mode === 'oidc' && (providers.length === 0 || !form.oidc_provider_key)
}

function getErrorMessage(error: unknown) {
  return error instanceof Error ? error.message : 'Something went wrong'
}
