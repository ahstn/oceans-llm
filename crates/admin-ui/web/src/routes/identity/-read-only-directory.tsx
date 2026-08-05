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
          <p className="text-sm text-[var(--color-text-muted)]">No users are available.</p>
        ) : (
          <div className="grid gap-3 lg:grid-cols-2">
            {users.map((user) => (
              <article
                key={user.id}
                className="flex flex-col gap-4 rounded-lg border border-[color:var(--color-border)] p-4"
              >
                <div className="flex min-w-0 items-start gap-3">
                  <GeneratedAvatar kind="user" name={user.name} size={40} />
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <h2 className="truncate font-semibold text-[var(--color-text)]">
                        {user.name}
                      </h2>
                      <Badge variant="outline">{formatRole(user.global_role)}</Badge>
                      <Badge>{user.status}</Badge>
                    </div>
                    <p className="truncate text-sm text-[var(--color-text-muted)]">{user.email}</p>
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
          <p className="text-sm text-[var(--color-text-muted)]">No teams are available.</p>
        ) : (
          <div className="grid gap-3 lg:grid-cols-2">
            {teams.map((team) => (
              <article
                key={team.id}
                className="flex flex-col gap-4 rounded-lg border border-[color:var(--color-border)] p-4"
              >
                <div className="flex min-w-0 items-start gap-3">
                  <GeneratedAvatar kind="team" name={team.name} size={40} />
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <h2 className="truncate font-semibold text-[var(--color-text)]">
                        {team.name}
                      </h2>
                      <Badge>{team.status}</Badge>
                    </div>
                  </div>
                  <span className="text-sm text-[var(--color-text-muted)]">
                    {formatMemberCount(team.member_count)}
                  </span>
                </div>
                <div className="flex flex-col gap-2">
                  <h3 className="text-xs font-medium tracking-wide text-[var(--color-text-soft)] uppercase">
                    Members
                  </h3>
                  {team.members.length === 0 ? (
                    <p className="text-sm text-[var(--color-text-muted)]">No members</p>
                  ) : (
                    <ul className="flex flex-col divide-y divide-[color:var(--color-border)]">
                      {team.members.map((member) => (
                        <li
                          key={member.id}
                          className="flex items-center justify-between gap-3 py-2"
                        >
                          <div className="min-w-0">
                            <p className="truncate text-sm font-medium text-[var(--color-text)]">
                              {member.name}
                            </p>
                            <p className="truncate text-xs text-[var(--color-text-muted)]">
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
      <dt className="text-xs text-[var(--color-text-soft)]">{label}</dt>
      <dd className="text-[var(--color-text)]">{value}</dd>
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
