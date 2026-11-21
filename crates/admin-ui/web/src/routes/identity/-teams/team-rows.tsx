// One team in the ledger: its table row, the detail panel its expansion reveals, and the menus
// that hold its actions. The mobile card in `teams-list.tsx` reuses the same detail and menus.

import type { ReactNode, SyntheticEvent } from 'react'
import {
  ArrowDown01Icon,
  ArrowRight01Icon,
  Copy01Icon,
  MoreHorizontalIcon,
  PlusSignIcon,
} from '@hugeicons/core-free-icons'
import { Link } from '@tanstack/react-router'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
import { AdminTableHeader } from '@/components/shared/admin-table-header'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { formatCount } from '@/lib/format'
import type { EntityTag } from '@/routes/identity/-entity-tag-utils'
import { EntityTagChips } from '@/routes/identity/-entity-tags'
import { statusBadgeVariant } from '@/routes/identity/-status-badge'
import type { TeamManagementView, TeamMemberView } from '@/types/api'

/** What a team's controls open: the team editor, `Add members`, and a member's transfer or removal. */
export interface TeamListActions {
  onEditTeam: (team: TeamManagementView) => void
  onAddMembers: (team: TeamManagementView) => void
  onTransferMember: (teamId: string, userId: string) => void
  onRemoveMember: (teamId: string, userId: string) => void
}

export const TEAM_TABLE_COLUMN_COUNT = 6

async function copyTeamKey(key: string) {
  try {
    await navigator.clipboard.writeText(key)
    toast.success('Team key copied')
  } catch {
    toast.error('Clipboard access failed')
  }
}

/** Controls inside a clickable row stop their clicks, including portalled menus, reaching it. */
function stopRowToggle(event: SyntheticEvent) {
  event.stopPropagation()
}

export function TeamTableRow({
  team,
  isExpanded,
  canOpenUsers,
  detailId,
  onToggle,
  onSelectTag,
  actions,
}: {
  team: TeamManagementView
  isExpanded: boolean
  canOpenUsers: boolean
  detailId: string
  onToggle: () => void
  onSelectTag: (tag: EntityTag) => void
  actions: TeamListActions
}) {
  return (
    // The whole row toggles its details, as the pull request ledger does.
    <TableRow
      className="group hover:bg-surface-muted cursor-pointer"
      data-state={isExpanded ? 'selected' : undefined}
      onClick={onToggle}
    >
      <TableCell className="px-3 py-2">
        <div className="flex min-w-0 items-center gap-3">
          <GeneratedAvatar kind="team" name={team.name} size={28} />
          <div className="flex min-w-0 flex-col">
            <span className="text-foreground truncate font-medium">{team.name}</span>
            <span className="text-muted-foreground truncate font-mono text-xs">{team.key}</span>
          </div>
        </div>
      </TableCell>
      <TableCell className="w-52 px-3 py-2">
        <TeamAdminsSummary team={team} canOpenUsers={canOpenUsers} />
      </TableCell>
      <TableCell className="w-40 px-3 py-2">
        <MemberStack members={team.members} count={team.member_count} />
      </TableCell>
      <TableCell className="w-88 px-3 py-2" onClick={stopRowToggle}>
        <EntityTagChips tags={team.tags} onSelect={onSelectTag} />
      </TableCell>
      <TableCell className="w-24 px-3 py-2">
        <Badge variant={statusBadgeVariant(team.status)}>{team.status}</Badge>
      </TableCell>
      <TableCell
        className="bg-card shadow-sticky-edge-end group-hover:bg-surface-muted group-data-[state=selected]:bg-muted sticky right-0 z-10 w-20 px-2 py-2"
        onClick={stopRowToggle}
      >
        <div className="flex items-center justify-end gap-0.5">
          <TeamActionsMenu team={team} actions={actions} />
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-expanded={isExpanded}
            aria-controls={detailId}
            aria-label={`${isExpanded ? 'Collapse' : 'Expand'} ${team.name}`}
            onClick={onToggle}
          >
            <AppIcon icon={isExpanded ? ArrowDown01Icon : ArrowRight01Icon} size={16} aria-hidden />
          </Button>
        </div>
      </TableCell>
    </TableRow>
  )
}

/** The expansion row under a team: members, tags, admins and key, as on the PR ledger. */
export function TeamDetailRow({ id, children }: { id: string; children: ReactNode }) {
  return (
    <TableRow className="hover:bg-transparent">
      <TableCell
        id={id}
        colSpan={TEAM_TABLE_COLUMN_COUNT}
        className="bg-muted/40 p-0 whitespace-normal"
      >
        {children}
      </TableCell>
    </TableRow>
  )
}

/** Edit team, Add members and Copy team key, in the row's pinned action column. */
export function TeamActionsMenu({
  team,
  actions,
}: {
  team: TeamManagementView
  actions: TeamListActions
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="text-muted-foreground"
          aria-label={`Actions for ${team.name}`}
        >
          <AppIcon icon={MoreHorizontalIcon} size={16} aria-hidden />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" onClick={stopRowToggle}>
        <DropdownMenuItem onSelect={() => actions.onEditTeam(team)}>Edit team</DropdownMenuItem>
        <DropdownMenuItem onSelect={() => actions.onAddMembers(team)}>Add members</DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => void copyTeamKey(team.key)}>
          Copy team key
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** The first admin as a link, with a count for the rest; `No admins` otherwise. */
function TeamAdminsSummary({
  team,
  canOpenUsers,
}: {
  team: TeamManagementView
  canOpenUsers: boolean
}) {
  const [first, ...rest] = team.admins
  if (!first) return <span className="text-muted-foreground text-xs">No admins</span>
  return (
    <div className="flex min-w-0 items-center gap-1">
      <UserEntityLink user={first} canOpenUsers={canOpenUsers} />
      {rest.length > 0 ? (
        <span
          className="text-muted-foreground shrink-0 text-xs tabular-nums"
          title={rest.map((admin) => admin.name).join(', ')}
        >
          +{rest.length}
        </span>
      ) : null}
    </div>
  )
}

const STACK_LIMIT = 4

/** Up to four member avatars, overlapped, then the count of the rest. */
export function MemberStack({ members, count }: { members: TeamMemberView[]; count: number }) {
  if (count === 0) return <span className="text-muted-foreground text-xs">No members</span>
  const extra = count - Math.min(members.length, STACK_LIMIT)
  return (
    <div className="flex items-center gap-2">
      <span className="flex items-center" title={members.map((member) => member.name).join(', ')}>
        {members.slice(0, STACK_LIMIT).map((member, index) => (
          <GeneratedAvatar
            key={member.id}
            kind="user"
            name={member.name || member.email}
            size={22}
            className={index > 0 ? 'ring-card -ml-1.5 ring-2' : 'ring-card ring-2'}
          />
        ))}
      </span>
      {extra > 0 ? (
        <span className="text-muted-foreground text-xs tabular-nums">+{formatCount(extra)}</span>
      ) : null}
      <span className="sr-only">
        {formatCount(count)} {count === 1 ? 'member' : 'members'}
      </span>
    </div>
  )
}

/**
 * Members in a nested table beside a rail with every tag, the admins and the team key. The rail
 * drops below the table on narrow screens.
 */
export function TeamDetail({
  team,
  canOpenUsers,
  actions,
}: {
  team: TeamManagementView
  canOpenUsers: boolean
  actions: TeamListActions
}) {
  return (
    <div className="grid min-w-0 gap-5 p-4 lg:grid-cols-[minmax(0,1fr)_17rem]">
      <div className="flex min-w-0 flex-col gap-2">
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-sm font-medium">
            Members{' '}
            <span className="text-muted-foreground font-normal tabular-nums">
              · {formatCount(team.member_count)}
            </span>
          </h3>
          {team.members.length > 0 ? <AddMembersButton team={team} actions={actions} /> : null}
        </div>
        {team.members.length === 0 ? (
          <div className="border-border flex flex-col items-center gap-2 rounded-md border border-dashed px-4 py-8 text-center">
            <p className="text-sm">No members assigned yet.</p>
            <AddMembersButton team={team} actions={actions} />
          </div>
        ) : (
          <MemberTable team={team} actions={actions} />
        )}
      </div>

      <aside className="lg:border-border flex min-w-0 flex-col gap-4 lg:border-l lg:pl-5">
        <DetailSection title="Tags">
          {team.tags.length === 0 ? (
            <p className="text-muted-foreground text-xs">
              No tags. Edit the team to add up to five.
            </p>
          ) : (
            <dl className="flex flex-col gap-1.5">
              {team.tags.map((tag) => (
                <div
                  key={tag.key}
                  className="grid grid-cols-[6.5rem_minmax(0,1fr)] items-center gap-2 font-mono text-xs"
                >
                  <dt className="text-muted-foreground truncate">{tag.key}</dt>
                  <dd className="truncate" title={tag.value}>
                    {tag.value}
                  </dd>
                </div>
              ))}
            </dl>
          )}
        </DetailSection>
        <DetailSection title="Admins">
          {team.admins.length === 0 ? (
            <p className="text-muted-foreground text-xs">
              No admins. Platform admins manage this team.
            </p>
          ) : (
            <div className="flex flex-col items-start gap-1">
              {team.admins.map((admin) => (
                <UserEntityLink key={admin.id} user={admin} canOpenUsers={canOpenUsers} />
              ))}
            </div>
          )}
        </DetailSection>
        <DetailSection title="Team key">
          <div className="flex min-w-0 items-center justify-between gap-2">
            <code className="truncate font-mono text-xs">{team.key}</code>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              className="text-muted-foreground"
              aria-label={`Copy team key ${team.key}`}
              onClick={() => void copyTeamKey(team.key)}
            >
              <AppIcon icon={Copy01Icon} size={14} aria-hidden />
            </Button>
          </div>
        </DetailSection>
      </aside>
    </div>
  )
}

function DetailSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex min-w-0 flex-col gap-2">
      <h3 className="text-muted-foreground text-xs font-medium">{title}</h3>
      {children}
    </section>
  )
}

function AddMembersButton({
  team,
  actions,
}: {
  team: TeamManagementView
  actions: TeamListActions
}) {
  return (
    <Button type="button" size="sm" variant="outline" onClick={() => actions.onAddMembers(team)}>
      <AppIcon icon={PlusSignIcon} size={14} aria-hidden data-icon="inline-start" />
      Add members
    </Button>
  )
}

function MemberTable({ team, actions }: { team: TeamManagementView; actions: TeamListActions }) {
  return (
    <div className="border-border bg-card overflow-hidden rounded-md border">
      <Table aria-label={`Members of ${team.name}`} className="text-left">
        <AdminTableHeader columns={['Member', 'Role', 'Status', { label: '', key: 'actions' }]} />
        <TableBody>
          {team.members.map((member) => (
            <TableRow key={member.id}>
              <TableCell className="px-3 py-1.5">
                <div className="flex min-w-0 items-center gap-2.5">
                  <GeneratedAvatar kind="user" name={member.name || member.email} size={24} />
                  <div className="flex min-w-0 flex-col">
                    <span className="truncate font-medium">{member.name}</span>
                    <span className="text-muted-foreground truncate text-xs">{member.email}</span>
                  </div>
                </div>
              </TableCell>
              <TableCell className="w-28 px-3 py-1.5">
                <Badge variant={member.role === 'owner' ? 'secondary' : 'outline'}>
                  {member.role}
                </Badge>
              </TableCell>
              <TableCell className="w-28 px-3 py-1.5">
                <Badge variant={statusBadgeVariant(member.status)}>{member.status}</Badge>
              </TableCell>
              <TableCell className="w-12 px-2 py-1.5 text-right">
                <MemberActionsMenu teamId={team.id} member={member} actions={actions} />
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

/** Transfer and Remove for one member. Owner memberships keep both disabled and say why. */
function MemberActionsMenu({
  teamId,
  member,
  actions,
}: {
  teamId: string
  member: TeamMemberView
  actions: TeamListActions
}) {
  const isOwner = member.role === 'owner'
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="text-muted-foreground"
          aria-label={`Actions for ${member.name}`}
        >
          <AppIcon icon={MoreHorizontalIcon} size={16} aria-hidden />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {isOwner ? (
          <>
            <DropdownMenuLabel className="text-muted-foreground max-w-56 text-xs font-normal">
              Owner memberships cannot be removed or transferred in this slice.
            </DropdownMenuLabel>
            <DropdownMenuSeparator />
          </>
        ) : null}
        <DropdownMenuItem
          disabled={isOwner}
          onSelect={() => actions.onTransferMember(teamId, member.id)}
        >
          Transfer
        </DropdownMenuItem>
        <DropdownMenuItem
          variant="destructive"
          disabled={isOwner}
          onSelect={() => actions.onRemoveMember(teamId, member.id)}
        >
          Remove
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** A team admin, linking to their `Manage user` dialog when the viewer can open the users page. */
function UserEntityLink({
  user,
  canOpenUsers,
}: {
  user: { id: string; name: string; email: string }
  canOpenUsers: boolean
}) {
  const content = (
    <>
      <GeneratedAvatar kind="user" name={user.name || user.email} size={20} />
      <span className="truncate">{user.name}</span>
    </>
  )
  if (!canOpenUsers) {
    return <span className="inline-flex min-w-0 items-center gap-2 px-1.5 py-0.5">{content}</span>
  }
  return (
    <Button
      asChild
      type="button"
      size="sm"
      variant="ghost"
      className="h-auto min-w-0 justify-start px-1.5 py-0.5 font-normal"
    >
      <Link
        to="/identity/users"
        search={{ user_id: user.id, user_section: 'overview' }}
        aria-label={`Open ${user.name}`}
        onClick={stopRowToggle}
      >
        {content}
      </Link>
    </Button>
  )
}
