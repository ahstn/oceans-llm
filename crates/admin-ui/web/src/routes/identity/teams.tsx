import { useMemo, useState, useTransition, type FormEvent } from 'react'
import { UserIcon } from '@hugeicons/core-free-icons'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
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
  removeIdentityTeamMember,
  transferIdentityTeamMember,
  updateIdentityTeam,
} from '@/server/admin-data.functions'
import type {
  CreateTeamInput,
  CreateUserInput,
  CreateUserResult,
  IdentityTeamsPayload,
  TeamAssignableUserView,
  TeamManagementView,
  TransferTeamMemberInput,
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

type TeamDialogState = { mode: 'closed' } | { mode: 'create' } | { mode: 'edit'; teamId: string }

type MembersDialogState = { mode: 'closed' } | { mode: 'open'; teamId: string }

type TeamMemberDialogState =
  | { mode: 'closed' }
  | { mode: 'remove'; teamId: string; userId: string }
  | { mode: 'transfer'; teamId: string; userId: string }

export function TeamsPage() {
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
  const [memberDialog, setMemberDialog] = useState<TeamMemberDialogState>({ mode: 'closed' })
  const [transferForm, setTransferForm] = useState<TransferTeamMemberInput>({
    destination_team_id: '',
    destination_role: 'member',
  })
  const [isPending, startTransition] = useTransition()

  const editingTeam =
    teamDialog.mode === 'edit'
      ? (teams.find((team) => team.id === teamDialog.teamId) ?? null)
      : null
  const membersTeam =
    membersDialog.mode === 'open'
      ? (teams.find((team) => team.id === membersDialog.teamId) ?? null)
      : null
  const memberDialogTeam =
    memberDialog.mode === 'closed'
      ? null
      : (teams.find((team) => team.id === memberDialog.teamId) ?? null)
  const memberDialogUser =
    memberDialog.mode === 'closed'
      ? null
      : (users.find((user) => user.id === memberDialog.userId) ?? null)
  const teamMembersByTeam = useMemo(
    () =>
      teams.map((team) => ({
        team,
        members: users.filter((user) => user.team_id === team.id),
      })),
    [teams, users],
  )

  const adminOptions = useMemo(
    () =>
      users.map((user) => ({
        user,
        disabled: user.team_id !== null && (editingTeam ? user.team_id !== editingTeam.id : true),
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
        disabled: user.team_id !== null && (!membersTeam || user.team_id !== membersTeam.id),
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

  function openRemoveMemberDialog(teamId: string, userId: string) {
    setMemberDialog({ mode: 'remove', teamId, userId })
  }

  function openTransferMemberDialog(teamId: string, userId: string) {
    setMemberDialog({ mode: 'transfer', teamId, userId })
    setTransferForm({
      destination_team_id: '',
      destination_role: 'member',
    })
  }

  function closeMemberDialog() {
    setMemberDialog({ mode: 'closed' })
    setTransferForm({
      destination_team_id: '',
      destination_role: 'member',
    })
  }

  function setInviteAuthMode(authMode: CreateUserInput['auth_mode']) {
    setInviteForm((current) => ({
      ...current,
      auth_mode: authMode,
      oidc_provider_key:
        authMode === 'oidc'
          ? (current.oidc_provider_key ??
            (oidcProviders.length === 1 ? oidcProviders[0].key : null))
          : null,
    }))
  }

  async function handleRemoveMember() {
    if (memberDialog.mode !== 'remove') {
      return
    }

    startTransition(async () => {
      try {
        await removeIdentityTeamMember({
          data: { teamId: memberDialog.teamId, userId: memberDialog.userId },
        })
        toast.success('Member removed')
        await refreshTeams()
        closeMemberDialog()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleTransferMember() {
    if (memberDialog.mode !== 'transfer' || !memberDialogTeam) {
      return
    }

    startTransition(async () => {
      try {
        await transferIdentityTeamMember({
          data: {
            teamId: memberDialog.teamId,
            userId: memberDialog.userId,
            input: sanitizeTransferForm(transferForm, memberDialogTeam),
          },
        })
        toast.success('Member transferred')
        await refreshTeams()
        closeMemberDialog()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
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
          {teams.length === 0 ? (
            <Empty>
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <AppIcon icon={UserIcon} size={22} stroke={1.5} />
                </EmptyMedia>
                <EmptyTitle>No teams created yet</EmptyTitle>
                <EmptyDescription>
                  Create the first team, assign admins if you already have users, and return later
                  to add more members as onboarding begins.
                </EmptyDescription>
              </EmptyHeader>
              <EmptyContent>
                <Button type="button" onClick={openCreateTeamDialog}>
                  Create first team
                </Button>
              </EmptyContent>
            </Empty>
          ) : (
            <div className="flex flex-col gap-4">
              <div className="grid gap-3 md:hidden">
                {teamMembersByTeam.map(({ team, members }) => (
                  <article
                    key={team.id}
                    className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4"
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <p className="truncate font-semibold text-[var(--color-text)]">
                          {team.name}
                        </p>
                        <p className="truncate text-xs text-[var(--color-text-soft)]">{team.key}</p>
                      </div>
                      <Badge variant={team.status === 'active' ? 'success' : 'warning'}>
                        {team.status}
                      </Badge>
                    </div>

                    <dl className="mt-4 grid grid-cols-2 gap-x-4 gap-y-3 text-sm">
                      <div>
                        <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                          Members
                        </dt>
                        <dd className="text-[var(--color-text-muted)]">{team.member_count}</dd>
                      </div>
                      <div>
                        <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                          Admins
                        </dt>
                        <dd className="text-[var(--color-text-muted)]">
                          {team.admins.length > 0 ? team.admins.length : 'None'}
                        </dd>
                      </div>
                    </dl>

                    <div className="mt-4 flex flex-wrap gap-2">
                      {team.admins.length > 0 ? (
                        team.admins.map((admin) => <Badge key={admin.id}>{admin.name}</Badge>)
                      ) : (
                        <span className="text-xs text-[var(--color-text-soft)]">
                          No admins assigned
                        </span>
                      )}
                    </div>

                    <div className="mt-4 flex flex-col gap-3">
                      <div className="flex items-center justify-between gap-2">
                        <h3 className="text-sm font-semibold text-[var(--color-text)]">Members</h3>
                        <span className="text-xs text-[var(--color-text-soft)]">
                          {members.length} total
                        </span>
                      </div>
                      {members.length > 0 ? (
                        <div className="flex flex-col gap-2">
                          {members.map((member) => (
                            <div
                              key={member.id}
                              className="rounded-md border border-[color:var(--color-border)] bg-[color:var(--color-bg)] p-3"
                            >
                              <div className="flex items-start justify-between gap-3">
                                <div className="min-w-0">
                                  <p className="truncate text-sm font-semibold text-[var(--color-text)]">
                                    {member.name}
                                  </p>
                                  <p className="truncate text-xs text-[var(--color-text-soft)]">
                                    {member.email}
                                  </p>
                                </div>
                                <Badge
                                  variant={member.team_role === 'owner' ? 'warning' : 'default'}
                                >
                                  {member.team_role ?? 'member'}
                                </Badge>
                              </div>
                              <div className="mt-3 flex flex-wrap gap-2">
                                <Button
                                  type="button"
                                  size="sm"
                                  variant="secondary"
                                  onClick={() => openTransferMemberDialog(team.id, member.id)}
                                  disabled={member.team_role === 'owner'}
                                >
                                  Transfer
                                </Button>
                                <Button
                                  type="button"
                                  size="sm"
                                  variant="ghost"
                                  onClick={() => openRemoveMemberDialog(team.id, member.id)}
                                  disabled={member.team_role === 'owner'}
                                >
                                  Remove
                                </Button>
                              </div>
                              {member.team_role === 'owner' ? (
                                <p className="mt-2 text-xs text-[var(--color-text-soft)]">
                                  Owner memberships cannot be removed or transferred in this slice.
                                </p>
                              ) : null}
                            </div>
                          ))}
                        </div>
                      ) : (
                        <p className="text-sm text-[var(--color-text-soft)]">
                          No members assigned yet.
                        </p>
                      )}
                    </div>

                    <div className="mt-4 flex flex-wrap gap-2">
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
                  </article>
                ))}
              </div>

              <div className="hidden overflow-hidden rounded-md border border-[color:var(--color-border)] md:block">
                <table className="w-full text-left text-sm">
                  <thead className="bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]">
                    <tr>
                      <th className="px-3 py-2 font-semibold">Team</th>
                      <th className="px-3 py-2 font-semibold">Admins</th>
                      <th className="px-3 py-2 font-semibold">Members</th>
                      <th className="px-3 py-2 font-semibold">Status</th>
                      <th className="px-3 py-2 font-semibold">Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {teamMembersByTeam.map(({ team, members }) => (
                      <tr
                        key={team.id}
                        className="border-t border-[color:var(--color-border)] align-top"
                      >
                        <td className="px-3 py-3">
                          <div className="flex flex-col gap-1">
                            <p className="text-[var(--color-text)]">{team.name}</p>
                            <p className="text-xs text-[var(--color-text-soft)]">{team.key}</p>
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
                            <span className="text-xs text-[var(--color-text-soft)]">No admins</span>
                          )}
                        </td>
                        <td className="px-3 py-3">
                          <div className="flex flex-col gap-2">
                            <div className="text-sm text-[var(--color-text-muted)]">
                              {team.member_count} members
                            </div>
                            {members.length > 0 ? (
                              <div className="flex flex-col gap-2">
                                {members.map((member) => (
                                  <div
                                    key={member.id}
                                    className="flex flex-wrap items-center gap-2 rounded-md border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] px-2 py-2"
                                  >
                                    <div className="min-w-0 flex-1">
                                      <p className="truncate text-[var(--color-text)]">
                                        {member.name}
                                      </p>
                                      <p className="truncate text-xs text-[var(--color-text-soft)]">
                                        {member.email}
                                      </p>
                                    </div>
                                    <Badge
                                      variant={member.team_role === 'owner' ? 'warning' : 'default'}
                                    >
                                      {member.team_role ?? 'member'}
                                    </Badge>
                                    <Button
                                      type="button"
                                      size="sm"
                                      variant="secondary"
                                      onClick={() => openTransferMemberDialog(team.id, member.id)}
                                      disabled={member.team_role === 'owner'}
                                    >
                                      Transfer
                                    </Button>
                                    <Button
                                      type="button"
                                      size="sm"
                                      variant="ghost"
                                      onClick={() => openRemoveMemberDialog(team.id, member.id)}
                                      disabled={member.team_role === 'owner'}
                                    >
                                      Remove
                                    </Button>
                                  </div>
                                ))}
                              </div>
                            ) : (
                              <span className="text-sm text-[var(--color-text-soft)]">
                                No members assigned yet
                              </span>
                            )}
                          </div>
                        </td>
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
            </div>
          )}
        </CardContent>
      </Card>

      <Dialog
        open={teamDialog.mode !== 'closed'}
        onOpenChange={(open) => !open && closeTeamDialog()}
      >
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
              <section className="flex flex-col gap-4 rounded-lg border border-[color:var(--color-border)] p-5">
                <div className="flex flex-col gap-1">
                  <h3 className="text-base font-semibold text-[var(--color-text)]">
                    Existing users
                  </h3>
                  <p className="text-sm text-[var(--color-text-muted)]">
                    Only teamless users can be newly added. Users already on another team remain
                    unavailable in this flow.
                  </p>
                </div>

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
              </section>

              <section className="flex flex-col gap-4 rounded-lg border border-[color:var(--color-border)] p-5">
                <div className="flex flex-col gap-1">
                  <h3 className="text-base font-semibold text-[var(--color-text)]">
                    Invite a new member
                  </h3>
                  <p className="text-sm text-[var(--color-text-muted)]">
                    This uses the same onboarding flow as the users page and preassigns the new user
                    to {membersTeam.name} as a member.
                  </p>
                </div>

                <div className="flex flex-col gap-4">
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
                          <InputGroup>
                            <InputGroupInput
                              id="generated-member-url"
                              readOnly
                              value={
                                inviteResult.kind === 'password_invite'
                                  ? inviteResult.invite_url
                                  : inviteResult.sign_in_url
                              }
                            />
                            <InputGroupAddon align="inline-end">
                              <Button
                                type="button"
                                variant="ghost"
                                size="sm"
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
                            </InputGroupAddon>
                          </InputGroup>
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
                </div>
              </section>

              <DialogFooter>
                <Button type="button" variant="secondary" onClick={closeMembersDialog}>
                  Close
                </Button>
              </DialogFooter>
            </div>
          ) : null}
        </DialogContent>
      </Dialog>

      <Dialog
        open={memberDialog.mode === 'remove'}
        onOpenChange={(open) => !open && closeMemberDialog()}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Remove member</DialogTitle>
            <DialogDescription>
              {memberDialogTeam && memberDialogUser
                ? `Remove ${memberDialogUser.name} from ${memberDialogTeam.name}. This only changes future membership-derived access.`
                : 'Remove this member from the team.'}
            </DialogDescription>
          </DialogHeader>

          {memberDialogUser && memberDialogTeam ? (
            <div className="flex flex-col gap-4">
              <Alert>
                <AlertTitle>Membership removal</AlertTitle>
                <AlertDescription>
                  Historical logs, spend, and API-key ownership are not migrated by this action.
                </AlertDescription>
              </Alert>

              {memberDialogUser.team_role === 'owner' ? (
                <Alert>
                  <AlertTitle>Owner membership is locked</AlertTitle>
                  <AlertDescription>
                    Owner memberships cannot be removed in this slice.
                  </AlertDescription>
                </Alert>
              ) : null}

              <DialogFooter>
                <Button type="button" variant="secondary" onClick={closeMemberDialog}>
                  Cancel
                </Button>
                <Button
                  type="button"
                  variant="destructive"
                  onClick={handleRemoveMember}
                  disabled={isPending || memberDialogUser.team_role === 'owner'}
                >
                  Remove member
                </Button>
              </DialogFooter>
            </div>
          ) : null}
        </DialogContent>
      </Dialog>

      <Dialog
        open={memberDialog.mode === 'transfer'}
        onOpenChange={(open) => !open && closeMemberDialog()}
      >
        <DialogContent className="w-[min(760px,calc(100vw-32px))]">
          <DialogHeader>
            <DialogTitle>Transfer member</DialogTitle>
            <DialogDescription>
              {memberDialogTeam && memberDialogUser
                ? `Move ${memberDialogUser.name} out of ${memberDialogTeam.name} and into another team. Future access follows the destination membership only.`
                : 'Transfer this member to another team.'}
            </DialogDescription>
          </DialogHeader>

          {memberDialogUser && memberDialogTeam ? (
            <form
              className="flex flex-col gap-5"
              onSubmit={(event) => {
                event.preventDefault()
                handleTransferMember()
              }}
            >
              {memberDialogUser.team_role === 'owner' ? (
                <Alert>
                  <AlertTitle>Owner memberships are locked</AlertTitle>
                  <AlertDescription>
                    Owner memberships are not removable or transferable in this slice.
                  </AlertDescription>
                </Alert>
              ) : null}

              <FieldGroup>
                <Field>
                  <FieldLabel htmlFor="transfer-destination-team">Destination team</FieldLabel>
                  <Select
                    value={transferForm.destination_team_id || undefined}
                    onValueChange={(value) =>
                      setTransferForm((current) => ({
                        ...current,
                        destination_team_id: value,
                      }))
                    }
                  >
                    <SelectTrigger id="transfer-destination-team">
                      <SelectValue placeholder="Select destination team" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>
                        {teams
                          .filter((team) => team.id !== memberDialogTeam.id)
                          .map((team) => (
                            <SelectItem key={team.id} value={team.id}>
                              {team.name}
                            </SelectItem>
                          ))}
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                  <FieldDescription>
                    Teams already using the user are omitted. Pick the new ownership boundary
                    explicitly.
                  </FieldDescription>
                </Field>

                <Field>
                  <FieldLabel htmlFor="transfer-destination-role">Destination role</FieldLabel>
                  <Select
                    value={transferForm.destination_role}
                    onValueChange={(value: TransferTeamMemberInput['destination_role']) =>
                      setTransferForm((current) => ({ ...current, destination_role: value }))
                    }
                  >
                    <SelectTrigger id="transfer-destination-role">
                      <SelectValue placeholder="Select destination role" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>
                        <SelectItem value="member">Member</SelectItem>
                        <SelectItem value="admin">Admin</SelectItem>
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                </Field>
              </FieldGroup>

              <Alert>
                <AlertTitle>Transfer is membership-only</AlertTitle>
                <AlertDescription>
                  This does not migrate past request logs, spend, budgets, or API-key ownership.
                </AlertDescription>
              </Alert>

              <DialogFooter>
                <Button type="button" variant="secondary" onClick={closeMemberDialog}>
                  Cancel
                </Button>
                <Button
                  type="submit"
                  disabled={
                    isPending ||
                    !transferForm.destination_team_id ||
                    transferForm.destination_team_id === memberDialogTeam.id ||
                    memberDialogUser.team_role === 'owner'
                  }
                >
                  Transfer member
                </Button>
              </DialogFooter>
            </form>
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
            <span className="text-xs text-[var(--color-text-soft)]">▼</span>
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
                    <span className="w-4 text-[var(--color-text-soft)]">
                      {selectedUserIds.includes(user.id) ? '✓' : ''}
                    </span>
                    <div className="flex min-w-0 flex-1 flex-col gap-1">
                      <span className="truncate">{user.name}</span>
                      <span className="truncate text-xs text-[var(--color-text-soft)]">
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

function sanitizeTransferForm(
  form: TransferTeamMemberInput,
  team: TeamManagementView,
): TransferTeamMemberInput {
  return {
    destination_team_id:
      form.destination_team_id && form.destination_team_id !== team.id
        ? form.destination_team_id
        : '',
    destination_role: form.destination_role,
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
