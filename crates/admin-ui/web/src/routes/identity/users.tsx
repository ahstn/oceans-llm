import { useState } from 'react'
import { UserIcon } from '@hugeicons/core-free-icons'
import { createFileRoute, useRouter, type SearchSchemaInput } from '@tanstack/react-router'

import { AppIcon } from '@/components/icons/app-icon'
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
import { getUserDirectory, getUsers } from '@/server/admin-data.functions'
import { CreateUserDialog } from '@/routes/identity/-users/create-user-dialog'
import { ReadOnlyUsersDirectory } from '@/routes/identity/-read-only-directory'
import { useIdentityTransition } from '@/routes/identity/-use-identity-transition'
import { UserDetailsDialog } from '@/routes/identity/-users/user-details-dialog'
import { UsersList } from '@/routes/identity/-users/users-list'
import {
  normalizeUserSearch,
  type UserDetailsSection,
  type UsersSearch,
} from '@/routes/identity/-users/users-search'
import type { IdentityDirectoryUsersPayload, IdentityUsersPayload, UserView } from '@/types/api'

export const Route = createFileRoute('/identity/users')({
  validateSearch: (
    search: { user_id?: string; user_section?: UserDetailsSection } & SearchSchemaInput,
  ) => normalizeUserSearch(search),
  loader: ({ context }) =>
    isPlatformAdminSession(context.session) ? getUsers() : getUserDirectory(),
  component: UsersPage,
})

const emptyAdminUsers: IdentityUsersPayload['users'] = []
const emptyAdminTeams: IdentityUsersPayload['teams'] = []
const emptyOidcProviders: IdentityUsersPayload['oidc_providers'] = []
const emptyOauthProviders: IdentityUsersPayload['oauth_providers'] = []
const emptyCopilotUserProviders: IdentityUsersPayload['copilot_user_providers'] = []

const pageHeader = (
  <PageHeader
    section="Identity"
    title="Users"
    description="Review user accounts, team assignments, and access settings."
  />
)

export function UsersPage() {
  const { session } = Route.useRouteContext()
  const loaderData = Route.useLoaderData()
  const search = Route.useSearch()

  if (!isPlatformAdminSession(session)) {
    const directoryData = loaderData.data as IdentityDirectoryUsersPayload | undefined
    return (
      <div className="flex min-w-0 flex-1 flex-col gap-6">
        {pageHeader}
        <ReadOnlyUsersDirectory users={directoryData?.users ?? []} />
      </div>
    )
  }

  return (
    <UserAdministration
      data={loaderData.data as IdentityUsersPayload | undefined}
      search={search}
    />
  )
}

/** The platform admin view: the user list, the `Add user` dialog, and the `Manage user` dialog. */
function UserAdministration({
  data,
  search,
}: {
  data: IdentityUsersPayload | undefined
  search: UsersSearch
}) {
  const router = useRouter()
  const transition = useIdentityTransition()
  const [isCreateOpen, setIsCreateOpen] = useState(false)
  const { users, teams, oidcProviders, oauthProviders, copilotUserProviders } = adminPayload(data)
  const selectedUser = search.user_id
    ? (users.find((user) => user.id === search.user_id) ?? null)
    : null

  function openUserDialog(user: UserView) {
    void router.navigate({
      to: '/identity/users',
      search: { user_id: user.id, user_section: 'overview' },
    })
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      {pageHeader}

      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-4">
          <div className="flex flex-col gap-1">
            <CardTitle>User list</CardTitle>
            <CardDescription>Select a user to review account and access settings.</CardDescription>
          </div>

          <CreateUserDialog
            open={isCreateOpen}
            onOpenChange={setIsCreateOpen}
            teams={teams}
            oidcProviders={oidcProviders}
            oauthProviders={oauthProviders}
            transition={transition}
          />
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
                <Button type="button" onClick={() => setIsCreateOpen(true)}>
                  Create first user
                </Button>
              </EmptyContent>
            </Empty>
          ) : (
            <UsersList users={users} onManage={openUserDialog} />
          )}
        </CardContent>
      </Card>

      <UserDetailsDialog
        user={selectedUser}
        section={search.user_section}
        teams={teams}
        oidcProviders={oidcProviders}
        oauthProviders={oauthProviders}
        copilotUserProviders={copilotUserProviders}
        transition={transition}
      />
    </div>
  )
}

/** The admin payload with stable empty lists for anything missing. */
function adminPayload(data: IdentityUsersPayload | undefined) {
  return {
    users: data?.users ?? emptyAdminUsers,
    teams: data?.teams ?? emptyAdminTeams,
    oidcProviders: data?.oidc_providers ?? emptyOidcProviders,
    oauthProviders: data?.oauth_providers ?? emptyOauthProviders,
    copilotUserProviders: data?.copilot_user_providers ?? emptyCopilotUserProviders,
  }
}
