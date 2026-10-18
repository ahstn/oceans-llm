import { useState } from 'react'
import { UnfoldMoreDownIcon, UnfoldMoreUpIcon } from '@hugeicons/core-free-icons'
import { Link } from '@tanstack/react-router'

import { AppIcon } from '@/components/icons/app-icon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { EntityTagBadges } from '@/routes/identity/-entity-tags'
import { statusBadgeVariant } from '@/routes/identity/-status-badge'
import type { TeamManagementView, TeamMemberView } from '@/types/api'

/** What a team's controls open: the team editor, `Add members`, and a member's transfer or removal. */
export interface TeamListActions {
  onEditTeam: (team: TeamManagementView) => void
  onAddMembers: (team: TeamManagementView) => void
  onTransferMember: (teamId: string, userId: string) => void
  onRemoveMember: (teamId: string, userId: string) => void
}

interface TeamEntryProps {
  team: TeamManagementView
  isExpanded: boolean
  canOpenUsers: boolean
  onToggleMembers: (teamId: string) => void
  actions: TeamListActions
}

/**
 * Teams as cards on small screens and as a table from `md` up. Each team's member roster stays
 * collapsed until it is expanded, and both layouts share which rosters are expanded.
 */
export function TeamsList({
  teams,
  canOpenUsers,
  ...actions
}: TeamListActions & {
  teams: TeamManagementView[]
  canOpenUsers: boolean
}) {
  const [expandedTeamIds, setExpandedTeamIds] = useState<ReadonlySet<string>>(() => new Set())

  function toggleMembers(teamId: string) {
    setExpandedTeamIds((current) => {
      const next = new Set(current)
      if (next.has(teamId)) {
        next.delete(teamId)
      } else {
        next.add(teamId)
      }
      return next
    })
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-3 md:hidden">
        {teams.map((team) => (
          <TeamCard
            key={team.id}
            team={team}
            isExpanded={expandedTeamIds.has(team.id)}
            canOpenUsers={canOpenUsers}
            onToggleMembers={toggleMembers}
            actions={actions}
          />
        ))}
      </div>

      <div className="border-border hidden overflow-hidden rounded-md border md:block">
        <Table className="text-left">
          <TableHeader className="bg-surface-muted">
            <TableRow>
              <TableHead className="text-muted-foreground px-3 py-2 font-semibold">Team</TableHead>
              <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
                Admins
              </TableHead>
              <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
                Members
              </TableHead>
              <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
                Status
              </TableHead>
              <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
                Actions
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {teams.map((team) => (
              <TeamTableRows
                key={team.id}
                team={team}
                isExpanded={expandedTeamIds.has(team.id)}
                canOpenUsers={canOpenUsers}
                onToggleMembers={toggleMembers}
                actions={actions}
              />
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}

function TeamCard({ team, isExpanded, canOpenUsers, onToggleMembers, actions }: TeamEntryProps) {
  const membersRegionId = `team-members-mobile-${team.id}`

  return (
    <article className="border-border bg-surface-muted rounded-lg border p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <GeneratedAvatar kind="team" name={team.name} size={40} />
          <div className="min-w-0">
            <p className="text-foreground truncate font-semibold">{team.name}</p>
            <p className="text-muted-foreground truncate text-xs">{team.key}</p>
          </div>
        </div>
        <Badge variant={statusBadgeVariant(team.status)}>{team.status}</Badge>
      </div>

      <div className="mt-4 flex flex-wrap gap-2">
        <EntityTagBadges tags={team.tags} />
      </div>

      <dl className="mt-4 grid grid-cols-2 gap-x-4 gap-y-3 text-sm">
        <div>
          <dt className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
            Members
          </dt>
          <dd className="text-subtle-foreground">{team.member_count}</dd>
        </div>
        <div>
          <dt className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
            Admins
          </dt>
          <dd className="text-subtle-foreground">
            {team.admins.length > 0 ? team.admins.length : 'None'}
          </dd>
        </div>
      </dl>

      <div className="mt-4 flex flex-wrap gap-2">
        {team.admins.length > 0 ? (
          team.admins.map((admin) => (
            <UserEntityLink key={admin.id} user={admin} canOpenUsers={canOpenUsers} />
          ))
        ) : (
          <span className="text-muted-foreground text-xs">No admins assigned</span>
        )}
      </div>

      <div className="mt-4 flex flex-wrap gap-2">
        <MembersToggleButton
          team={team}
          isExpanded={isExpanded}
          regionId={membersRegionId}
          variant="secondary"
          onToggle={onToggleMembers}
        />
        <TeamActionButtons team={team} actions={actions} />
      </div>

      {isExpanded ? (
        <div id={membersRegionId} className="mt-4">
          <TeamMemberRoster
            teamId={team.id}
            members={team.members}
            onTransferMember={actions.onTransferMember}
            onRemoveMember={actions.onRemoveMember}
          />
        </div>
      ) : null}
    </article>
  )
}

function TeamTableRows({
  team,
  isExpanded,
  canOpenUsers,
  onToggleMembers,
  actions,
}: TeamEntryProps) {
  const membersRegionId = `team-members-desktop-${team.id}`

  return (
    <>
      <TableRow aria-expanded={isExpanded}>
        <TableCell className="px-3 py-3">
          <div className="flex min-w-0 items-center gap-3">
            <GeneratedAvatar kind="team" name={team.name} size={32} />
            <div className="flex min-w-0 flex-col gap-1">
              <p className="text-foreground truncate">{team.name}</p>
              <p className="text-muted-foreground truncate text-xs">{team.key}</p>
              <EntityTagBadges tags={team.tags} />
            </div>
          </div>
        </TableCell>
        <TableCell className="px-3 py-3">
          {team.admins.length > 0 ? (
            <div className="flex flex-wrap gap-2">
              {team.admins.map((admin) => (
                <UserEntityLink key={admin.id} user={admin} canOpenUsers={canOpenUsers} compact />
              ))}
            </div>
          ) : (
            <span className="text-muted-foreground text-xs">No admins</span>
          )}
        </TableCell>
        <TableCell className="px-3 py-3">
          <MembersToggleButton
            team={team}
            isExpanded={isExpanded}
            regionId={membersRegionId}
            variant="ghost"
            onToggle={onToggleMembers}
          />
        </TableCell>
        <TableCell className="px-3 py-3">
          <Badge variant={statusBadgeVariant(team.status)}>{team.status}</Badge>
        </TableCell>
        <TableCell className="px-3 py-3">
          <div className="flex flex-wrap gap-2">
            <TeamActionButtons team={team} actions={actions} />
          </div>
        </TableCell>
      </TableRow>
      {isExpanded ? (
        <TableRow>
          <TableCell
            id={membersRegionId}
            colSpan={5}
            className="bg-surface-muted px-3 py-4 whitespace-normal"
          >
            <TeamMemberRoster
              teamId={team.id}
              members={team.members}
              onTransferMember={actions.onTransferMember}
              onRemoveMember={actions.onRemoveMember}
            />
          </TableCell>
        </TableRow>
      ) : null}
    </>
  )
}

function MembersToggleButton({
  team,
  isExpanded,
  regionId,
  variant,
  onToggle,
}: {
  team: TeamManagementView
  isExpanded: boolean
  regionId: string
  variant: 'secondary' | 'ghost'
  onToggle: (teamId: string) => void
}) {
  return (
    <Button
      type="button"
      size="sm"
      variant={variant}
      aria-expanded={isExpanded}
      aria-controls={regionId}
      onClick={() => onToggle(team.id)}
    >
      <AppIcon
        icon={isExpanded ? UnfoldMoreUpIcon : UnfoldMoreDownIcon}
        stroke={1.5}
        aria-hidden
        data-icon="inline-start"
        className="transition-transform duration-200 ease-out"
      />
      <span>
        {isExpanded ? 'Hide' : 'Show'} <span className="tabular-nums">{team.member_count}</span>{' '}
        {team.member_count === 1 ? 'member' : 'members'}
      </span>
    </Button>
  )
}

function TeamActionButtons({
  team,
  actions,
}: {
  team: TeamManagementView
  actions: TeamListActions
}) {
  return (
    <>
      <Button type="button" size="sm" variant="secondary" onClick={() => actions.onEditTeam(team)}>
        Edit team
      </Button>
      <Button type="button" size="sm" variant="ghost" onClick={() => actions.onAddMembers(team)}>
        Add members
      </Button>
    </>
  )
}

/** A team admin, linking to their `Manage user` dialog when the viewer can open the users page. */
function UserEntityLink({
  user,
  canOpenUsers,
  compact = false,
}: {
  user: { id: string; name: string; email: string; status?: string }
  canOpenUsers: boolean
  compact?: boolean
}) {
  if (!canOpenUsers) {
    return (
      <span className="inline-flex items-center gap-2 px-2 py-1">
        <GeneratedAvatar kind="user" name={user.name || user.email} size={compact ? 20 : 24} />
        <span className="truncate">{user.name}</span>
      </span>
    )
  }

  return (
    <Button asChild type="button" size="sm" variant="secondary" className="h-auto px-2 py-1">
      <Link
        to="/identity/users"
        search={{ user_id: user.id, user_section: 'overview' }}
        aria-label={`Open ${user.name}`}
      >
        <GeneratedAvatar kind="user" name={user.name || user.email} size={compact ? 20 : 24} />
        <span className="truncate">{user.name}</span>
        {!compact && user.status ? (
          <span className="text-muted-foreground text-xs">{user.status}</span>
        ) : null}
      </Link>
    </Button>
  )
}

function TeamMemberRoster({
  teamId,
  members,
  onTransferMember,
  onRemoveMember,
}: {
  teamId: string
  members: TeamMemberView[]
  onTransferMember: (teamId: string, userId: string) => void
  onRemoveMember: (teamId: string, userId: string) => void
}) {
  if (members.length === 0) {
    return (
      <div className="border-border bg-background text-muted-foreground rounded-md border p-4 text-sm">
        No members assigned yet.
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-foreground text-sm font-semibold">Members</h3>
        <span className="text-muted-foreground text-xs">{members.length} total</span>
      </div>
      <div className="grid gap-2">
        {members.map((member) => (
          <div
            key={member.id}
            className="border-border bg-background flex flex-wrap items-center gap-2 rounded-md border px-3 py-2"
          >
            <GeneratedAvatar kind="user" name={member.name || member.email} size={32} />
            <div className="min-w-0 flex-1">
              <p className="text-foreground truncate text-sm font-medium">{member.name}</p>
              <p className="text-muted-foreground truncate text-xs">{member.email}</p>
            </div>
            <Badge variant={member.role === 'owner' ? 'secondary' : 'outline'}>{member.role}</Badge>
            <Button
              type="button"
              size="sm"
              variant="secondary"
              onClick={() => onTransferMember(teamId, member.id)}
              disabled={member.role === 'owner'}
            >
              Transfer
            </Button>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={() => onRemoveMember(teamId, member.id)}
              disabled={member.role === 'owner'}
            >
              Remove
            </Button>
            {member.role === 'owner' ? (
              <p className="text-muted-foreground basis-full text-xs">
                Owner memberships cannot be removed or transferred in this slice.
              </p>
            ) : null}
          </div>
        ))}
      </div>
    </div>
  )
}
