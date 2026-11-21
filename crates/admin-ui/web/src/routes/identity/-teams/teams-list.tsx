import { Fragment, useMemo, useState } from 'react'
import {
  Cancel01Icon,
  FilterHorizontalIcon,
  Search01Icon,
  Shield01Icon,
  Tag01Icon,
  UnfoldMoreDownIcon,
  UnfoldMoreUpIcon,
} from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { AdminTableHead, AdminTableHeader } from '@/components/shared/admin-table-header'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { Table, TableBody } from '@/components/ui/table'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { useIsMobile } from '@/hooks/use-mobile'
import { formatCount } from '@/lib/format'
import { cn } from '@/lib/utils'
import type { EntityTag } from '@/routes/identity/-entity-tag-utils'
import { EntityTagChips } from '@/routes/identity/-entity-tags'
import { statusBadgeVariant } from '@/routes/identity/-status-badge'
import type { TeamManagementView } from '@/types/api'

import {
  emptyTeamFilters,
  filterTeams,
  hasActiveFilters,
  tagFilterId,
  teamTagOptions,
  toggleTagFilter,
  type TeamFilters,
  type TeamStatusFilter,
} from './team-filters'
import {
  MemberStack,
  TeamActionsMenu,
  TeamDetail,
  TeamDetailRow,
  TeamTableRow,
  type TeamListActions,
} from './team-rows'

export type { TeamListActions } from './team-rows'

/**
 * Teams as a ledger, laid out like the merged pull requests on Attribution: a toolbar that filters by text, tag, admins and status, and a table whose rows expand to their
 * members. Small screens get cards that share the same filters and expanded state.
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
  const [filters, setFilters] = useState<TeamFilters>(emptyTeamFilters)
  // Cards and table both stay in the DOM, switched by CSS, but an expanded roster mounts only in
  // the visible layout. Rosters open on a click, after hydration, so this never changes SSR markup.
  const isMobile = useIsMobile()
  const visible = useMemo(() => filterTeams(teams, filters), [teams, filters])

  function toggleExpanded(teamId: string) {
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

  function addTagFilter(tag: EntityTag) {
    setFilters((current) =>
      current.tags.some((entry) => tagFilterId(entry) === tagFilterId(tag))
        ? current
        : toggleTagFilter(current, tag),
    )
  }

  return (
    <div className="flex min-w-0 flex-col gap-4">
      <TeamListToolbar teams={teams} filters={filters} onChange={setFilters} />

      {visible.length === 0 ? (
        <div className="border-border text-muted-foreground flex flex-col items-center gap-2 rounded-md border px-4 py-10 text-center text-sm">
          <p>No teams match these filters.</p>
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={() => setFilters(emptyTeamFilters)}
          >
            Clear filters
          </Button>
        </div>
      ) : (
        <>
          <div data-testid="team-cards" className="grid grid-cols-1 gap-3 md:hidden">
            {visible.map((team) => (
              <TeamCard
                key={team.id}
                team={team}
                isExpanded={expandedTeamIds.has(team.id)}
                showDetail={isMobile}
                canOpenUsers={canOpenUsers}
                onToggle={() => toggleExpanded(team.id)}
                onSelectTag={addTagFilter}
                actions={actions}
              />
            ))}
          </div>

          <div className="border-border hidden overflow-x-auto rounded-md border md:block">
            <Table aria-label="Teams" className="min-w-240 text-left">
              <AdminTableHeader>
                <AdminTableHead>Team</AdminTableHead>
                <AdminTableHead>Admins</AdminTableHead>
                <AdminTableHead>Members</AdminTableHead>
                <AdminTableHead>Tags</AdminTableHead>
                <AdminTableHead>Status</AdminTableHead>
                {/* Pinned right, so the actions stay in reach while the table scrolls sideways. */}
                <AdminTableHead
                  className="bg-surface-muted shadow-sticky-edge-end sticky right-0 z-10 w-20"
                  aria-label="Actions"
                />
              </AdminTableHeader>
              <TableBody>
                {visible.map((team) => {
                  const isExpanded = expandedTeamIds.has(team.id)
                  const detailId = `team-detail-${team.id}`
                  return (
                    <Fragment key={team.id}>
                      <TeamTableRow
                        team={team}
                        isExpanded={isExpanded}
                        canOpenUsers={canOpenUsers}
                        detailId={detailId}
                        onToggle={() => toggleExpanded(team.id)}
                        onSelectTag={addTagFilter}
                        actions={actions}
                      />
                      {isExpanded && !isMobile ? (
                        <TeamDetailRow id={detailId}>
                          <TeamDetail team={team} canOpenUsers={canOpenUsers} actions={actions} />
                        </TeamDetailRow>
                      ) : null}
                    </Fragment>
                  )
                })}
              </TableBody>
            </Table>
          </div>
        </>
      )}

      <p className="text-muted-foreground text-xs">
        Showing {formatCount(visible.length)} of {formatCount(teams.length)} teams
      </p>
    </div>
  )
}

/** Search, the Filter menu and its applied chips, and the status toggle. */
function TeamListToolbar({
  teams,
  filters,
  onChange,
}: {
  teams: TeamManagementView[]
  filters: TeamFilters
  onChange: (filters: TeamFilters) => void
}) {
  const tagOptions = useMemo(() => teamTagOptions(teams), [teams])
  const applied = new Set(filters.tags.map(tagFilterId))
  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <InputGroup className="w-full sm:w-60">
          <InputGroupInput
            type="search"
            aria-label="Search teams"
            placeholder="Search teams or keys…"
            value={filters.query}
            onChange={(event) => onChange({ ...filters, query: event.target.value })}
          />
          <InputGroupAddon>
            <AppIcon icon={Search01Icon} aria-hidden />
          </InputGroupAddon>
        </InputGroup>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button type="button" variant="outline" size="sm">
              <AppIcon icon={FilterHorizontalIcon} size={14} aria-hidden data-icon="inline-start" />
              Filter
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="max-h-80 w-60">
            <DropdownMenuCheckboxItem
              checked={filters.withoutAdmins}
              onCheckedChange={(checked) => onChange({ ...filters, withoutAdmins: checked })}
              onSelect={(event) => event.preventDefault()}
            >
              Without admins
            </DropdownMenuCheckboxItem>
            <DropdownMenuSeparator />
            <DropdownMenuLabel className="text-muted-foreground text-xs font-normal">
              Tags
            </DropdownMenuLabel>
            {tagOptions.length === 0 ? (
              <p className="text-muted-foreground px-1.5 py-1 text-xs">No team has tags yet.</p>
            ) : (
              tagOptions.map((tag) => (
                <DropdownMenuCheckboxItem
                  key={tagFilterId(tag)}
                  checked={applied.has(tagFilterId(tag))}
                  onCheckedChange={() => onChange(toggleTagFilter(filters, tag))}
                  onSelect={(event) => event.preventDefault()}
                  className="font-mono text-xs"
                >
                  <span className="text-muted-foreground">{tag.key}</span>
                  <span className="truncate">{tag.value}</span>
                </DropdownMenuCheckboxItem>
              ))
            )}
          </DropdownMenuContent>
        </DropdownMenu>
        {filters.withoutAdmins ? (
          <AppliedFilter
            icon={Shield01Icon}
            field="Admins"
            value="none"
            onRemove={() => onChange({ ...filters, withoutAdmins: false })}
          />
        ) : null}
        {filters.tags.map((tag) => (
          <AppliedFilter
            key={tagFilterId(tag)}
            icon={Tag01Icon}
            field={tag.key}
            value={tag.value}
            mono
            onRemove={() => onChange(toggleTagFilter(filters, tag))}
          />
        ))}
        {hasActiveFilters(filters) ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="text-muted-foreground"
            onClick={() => onChange(emptyTeamFilters)}
          >
            Clear
          </Button>
        ) : null}
      </div>
      <ToggleGroup
        type="single"
        variant="outline"
        size="sm"
        value={filters.status}
        onValueChange={(next) => next && onChange({ ...filters, status: next as TeamStatusFilter })}
        aria-label="Filter by status"
      >
        <ToggleGroupItem value="all">All</ToggleGroupItem>
        <ToggleGroupItem value="active">Active</ToggleGroupItem>
        <ToggleGroupItem value="inactive">Inactive</ToggleGroupItem>
      </ToggleGroup>
    </div>
  )
}

/** One applied filter as `field is value ×`, in the shape of ReUI's filter chips. */
function AppliedFilter({
  icon,
  field,
  value,
  mono = false,
  onRemove,
}: {
  icon: typeof Tag01Icon
  field: string
  value: string
  mono?: boolean
  onRemove: () => void
}) {
  return (
    // Long tag values truncate inside the chip; the remove button never shrinks out of view.
    <span
      className="border-input inline-flex h-7 max-w-full min-w-0 items-stretch overflow-hidden rounded-lg border text-xs"
      title={`${field} is ${value}`}
    >
      <span className="text-muted-foreground flex max-w-32 min-w-0 shrink-0 items-center gap-1.5 px-2">
        <AppIcon icon={icon} size={14} aria-hidden className="shrink-0" />
        <span className={cn('truncate', mono && 'font-mono')}>{field}</span>
      </span>
      <span className="border-input text-muted-foreground flex shrink-0 items-center border-l px-2">
        is
      </span>
      <span
        className={cn('border-input flex min-w-0 items-center border-l px-2', mono && 'font-mono')}
      >
        <span className="truncate">{value}</span>
      </span>
      <button
        type="button"
        className="border-input text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:ring-ring/50 flex shrink-0 items-center border-l px-1.5 transition-colors outline-none focus-visible:ring-3 focus-visible:ring-inset"
        aria-label={`Remove ${field} filter`}
        onClick={onRemove}
      >
        <AppIcon icon={Cancel01Icon} size={12} aria-hidden />
      </button>
    </span>
  )
}

function TeamCard({
  team,
  isExpanded,
  showDetail,
  canOpenUsers,
  onToggle,
  onSelectTag,
  actions,
}: {
  team: TeamManagementView
  isExpanded: boolean
  /** Whether this card is the visible layout, so its expanded roster should mount. */
  showDetail: boolean
  canOpenUsers: boolean
  onToggle: () => void
  onSelectTag: (tag: EntityTag) => void
  actions: TeamListActions
}) {
  const detailId = `team-detail-mobile-${team.id}`
  return (
    <article className="border-border bg-surface-muted flex flex-col gap-3 rounded-lg border p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <GeneratedAvatar kind="team" name={team.name} size={36} />
          <div className="flex min-w-0 flex-col">
            <p className="text-foreground truncate font-medium">{team.name}</p>
            <p className="text-muted-foreground truncate font-mono text-xs">{team.key}</p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Badge variant={statusBadgeVariant(team.status)}>{team.status}</Badge>
          <TeamActionsMenu team={team} actions={actions} />
        </div>
      </div>

      <EntityTagChips tags={team.tags} onSelect={onSelectTag} empty={null} />

      <div className="flex items-center justify-between gap-3">
        <MemberStack members={team.members} count={team.member_count} />
        <span className="text-muted-foreground text-xs">
          {team.admins.length > 0
            ? `${formatCount(team.admins.length)} ${team.admins.length === 1 ? 'admin' : 'admins'}`
            : 'No admins'}
        </span>
      </div>

      <Button
        type="button"
        size="sm"
        variant="secondary"
        aria-expanded={isExpanded}
        aria-controls={detailId}
        onClick={onToggle}
      >
        <AppIcon
          icon={isExpanded ? UnfoldMoreUpIcon : UnfoldMoreDownIcon}
          stroke={1.5}
          aria-hidden
          data-icon="inline-start"
        />
        {isExpanded ? 'Hide' : 'Show'} {formatCount(team.member_count)}{' '}
        {team.member_count === 1 ? 'member' : 'members'}
      </Button>

      {isExpanded && showDetail ? (
        <div id={detailId} className="border-border -mx-4 -mb-4 border-t">
          <TeamDetail team={team} canOpenUsers={canOpenUsers} actions={actions} />
        </div>
      ) : null}
    </article>
  )
}
