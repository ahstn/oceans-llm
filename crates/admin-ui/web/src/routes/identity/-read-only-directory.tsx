import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import type { IdentityDirectoryTeamView, IdentityDirectoryUserView } from '@/types/api'

export function ReadOnlyUsersDirectory({ users }: { users: IdentityDirectoryUserView[] }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>User list</CardTitle>
        <CardDescription>
          Review each user's account and team details. Only administrators can make changes.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {users.length === 0 ? (
          <p className="text-subtle-foreground text-sm">No users are available.</p>
        ) : (
          <div className="grid gap-3 lg:grid-cols-2">
            {users.map((user) => (
              <article
                key={user.id}
                className="border-border flex flex-col gap-4 rounded-lg border p-4"
              >
                <div className="flex min-w-0 items-start gap-3">
                  <GeneratedAvatar kind="user" name={user.name} size={40} />
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <h2 className="text-foreground truncate font-semibold">{user.name}</h2>
                      <Badge variant="outline">{formatRole(user.global_role)}</Badge>
                      <Badge>{user.status}</Badge>
                    </div>
                    <p className="text-subtle-foreground truncate text-sm">{user.email}</p>
                  </div>
                </div>
                <dl className="grid gap-3 text-sm sm:grid-cols-2">
                  <DirectoryDetail label="Team" value={user.team_name ?? 'No team'} />
                  <DirectoryDetail label="Team role" value={formatRole(user.team_role)} />
                </dl>
              </article>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  )
}

export function ReadOnlyTeamsDirectory({ teams }: { teams: IdentityDirectoryTeamView[] }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Team list</CardTitle>
        <CardDescription>
          Review each team and its members. Only administrators can make changes.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {teams.length === 0 ? (
          <p className="text-subtle-foreground text-sm">No teams are available.</p>
        ) : (
          <div className="grid gap-3 lg:grid-cols-2">
            {teams.map((team) => (
              <article
                key={team.id}
                className="border-border flex flex-col gap-4 rounded-lg border p-4"
              >
                <div className="flex min-w-0 items-start gap-3">
                  <GeneratedAvatar kind="team" name={team.name} size={40} />
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <h2 className="text-foreground truncate font-semibold">{team.name}</h2>
                      <Badge>{team.status}</Badge>
                    </div>
                  </div>
                  <span className="text-subtle-foreground text-sm">
                    {formatMemberCount(team.member_count)}
                  </span>
                </div>
                <div className="flex flex-col gap-2">
                  <h3 className="text-muted-foreground text-xs font-medium tracking-wide uppercase">
                    Members
                  </h3>
                  {team.members.length === 0 ? (
                    <p className="text-subtle-foreground text-sm">No members</p>
                  ) : (
                    <ul className="divide-border flex flex-col divide-y">
                      {team.members.map((member) => (
                        <li
                          key={member.id}
                          className="flex items-center justify-between gap-3 py-2"
                        >
                          <div className="min-w-0">
                            <p className="text-foreground truncate text-sm font-medium">
                              {member.name}
                            </p>
                            <p className="text-subtle-foreground truncate text-xs">
                              {member.email}
                            </p>
                          </div>
                          <Badge variant="outline">{formatRole(member.role)}</Badge>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              </article>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  )
}

function DirectoryDetail({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-muted-foreground text-xs">{label}</dt>
      <dd className="text-foreground">{value}</dd>
    </div>
  )
}

function formatRole(value: string | null) {
  if (!value) return 'None'
  return value.replaceAll('_', ' ').replace(/^./, (character) => character.toUpperCase())
}

function formatMemberCount(count: number) {
  return `${count} ${count === 1 ? 'member' : 'members'}`
}
