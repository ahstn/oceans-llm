import { UserIcon } from '@hugeicons/core-free-icons'
import { createFileRoute } from '@tanstack/react-router'

import { AppIcon } from '@/components/icons/app-icon'
import { canAccessPage } from '@/components/layout/admin-nav'
import { PageHeader } from '@/components/layout/page-header'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty'
import { isPlatformAdminSession } from '@/routes/-auth-routing'
import { getTeams, getTeamDirectory } from '@/server/admin-data.functions'
import { AddMembersDialog } from '@/routes/identity/-teams/add-members-dialog'
import {
  RemoveMemberDialog,
  TransferMemberDialog,
} from '@/routes/identity/-teams/membership-change-dialogs'
import { ReadOnlyTeamsDirectory } from '@/routes/identity/-read-only-directory'
import { TeamEditorDialog } from '@/routes/identity/-teams/team-editor-dialog'
import { TeamsList } from '@/routes/identity/-teams/teams-list'
import { useAddMembers } from '@/routes/identity/-teams/use-add-members'
import { useMembershipChange } from '@/routes/identity/-teams/use-membership-change'
import { useTeamEditor } from '@/routes/identity/-teams/use-team-editor'
import { useIdentityTransition } from '@/routes/identity/-use-identity-transition'
import type { IdentityDirectoryTeamsPayload, IdentityTeamsPayload } from '@/types/api'

export const Route = createFileRoute('/identity/teams')({
  loader: ({ context }) =>
    isPlatformAdminSession(context.session) ? getTeams() : getTeamDirectory(),
  component: TeamsPage,
})

const emptyAdminTeams: IdentityTeamsPayload['teams'] = []
const emptyAssignableUsers: IdentityTeamsPayload['users'] = []
const emptyOidcProviders: IdentityTeamsPayload['oidc_providers'] = []
const emptyOauthProviders: IdentityTeamsPayload['oauth_providers'] = []

const pageHeader = (
  <PageHeader
    section="Identity"
    title="Teams"
    description="Review teams, their administrators, and their members."
  />
)

export function TeamsPage() {
  const { session } = Route.useRouteContext()
  const loaderData = Route.useLoaderData()

  if (!isPlatformAdminSession(session)) {
    const directoryData = loaderData.data as IdentityDirectoryTeamsPayload | undefined
    return (
      <div className="flex min-w-0 flex-1 flex-col gap-6">
        {pageHeader}
        <ReadOnlyTeamsDirectory teams={directoryData?.teams ?? []} />
      </div>
    )
  }

  return (
    <TeamAdministration
      data={loaderData.data as IdentityTeamsPayload | undefined}
      canOpenUsers={Boolean(session && canAccessPage(session, 'users'))}
    />
  )
}

/**
 * The platform admin view: the team list and the dialogs it opens. Every action runs in one shared
 * transition, so a pending action disables the other dialogs' buttons.
 */
function TeamAdministration({
  data,
  canOpenUsers,
}: {
  data: IdentityTeamsPayload | undefined
  canOpenUsers: boolean
}) {
  const transition = useIdentityTransition()
  const { teams, users, oidcProviders, oauthProviders } = adminPayload(data)
  const teamEditor = useTeamEditor({ teams, transition })
  const addMembers = useAddMembers({ teams, oidcProviders, oauthProviders, transition })
  const membershipChange = useMembershipChange({ teams, users, transition })

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      {pageHeader}

      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-4">
          <div className="flex flex-col gap-1">
            <CardTitle>Team list</CardTitle>
            <CardDescription>Select a team to review its members and settings.</CardDescription>
          </div>
          <Button type="button" onClick={teamEditor.openCreate}>
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
                <Button type="button" onClick={teamEditor.openCreate}>
                  Create first team
                </Button>
              </EmptyContent>
            </Empty>
          ) : (
            <TeamsList
              teams={teams}
              users={users}
              canOpenUsers={canOpenUsers}
              onEditTeam={teamEditor.openEdit}
              onAddMembers={addMembers.open}
              onTransferMember={membershipChange.openTransfer}
              onRemoveMember={membershipChange.openRemove}
            />
          )}
        </CardContent>
      </Card>

      <TeamEditorDialog editor={teamEditor} users={users} isPending={transition.isPending} />
      <AddMembersDialog
        addMembers={addMembers}
        users={users}
        oidcProviders={oidcProviders}
        oauthProviders={oauthProviders}
        isPending={transition.isPending}
      />
      <RemoveMemberDialog change={membershipChange} isPending={transition.isPending} />
      <TransferMemberDialog
        change={membershipChange}
        teams={teams}
        isPending={transition.isPending}
      />
    </div>
  )
}

/** The admin payload with stable empty lists for anything missing. */
function adminPayload(data: IdentityTeamsPayload | undefined) {
  return {
    teams: data?.teams ?? emptyAdminTeams,
    users: data?.users ?? emptyAssignableUsers,
    oidcProviders: data?.oidc_providers ?? emptyOidcProviders,
    oauthProviders: data?.oauth_providers ?? emptyOauthProviders,
  }
}
