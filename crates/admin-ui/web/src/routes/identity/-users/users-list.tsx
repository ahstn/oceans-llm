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
import { statusBadgeVariant } from '@/routes/identity/-status-badge'
import type { UserView } from '@/types/api'

interface UserRowsProps {
  users: UserView[]
  onManage: (user: UserView) => void
}

/** Users as cards on small screens and as a table from `md` up. */
export function UsersList({ users, onManage }: UserRowsProps) {
  return (
    <div className="flex flex-col gap-4">
      <UserCards users={users} onManage={onManage} />
      <UsersTable users={users} onManage={onManage} />
    </div>
  )
}

function UserCards({ users, onManage }: UserRowsProps) {
  return (
    <div className="grid gap-3 md:hidden">
      {users.map((user) => (
        <article key={user.id} className="border-border bg-surface-muted rounded-lg border p-4">
          <div className="flex items-start justify-between gap-3">
            <div className="flex min-w-0 items-center gap-3">
              <GeneratedAvatar kind="user" name={user.name || user.email} size={40} />
              <div className="min-w-0">
                <p className="text-foreground truncate font-semibold">{user.name}</p>
                <p className="text-subtle-foreground truncate text-sm">{user.email}</p>
              </div>
            </div>
            <Badge variant={statusBadgeVariant(user.status)}>{user.status}</Badge>
          </div>

          <dl className="mt-4 grid grid-cols-2 gap-x-4 gap-y-3 text-sm">
            <div>
              <dt className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
                Global role
              </dt>
              <dd className="text-subtle-foreground">{user.global_role}</dd>
            </div>
            <div>
              <dt className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
                Team
              </dt>
              <dd className="text-subtle-foreground">{user.team_name ?? 'No team'}</dd>
            </div>
          </dl>

          <div className="mt-4 flex flex-wrap gap-2">
            <Button type="button" size="sm" variant="secondary" onClick={() => onManage(user)}>
              Manage
            </Button>
          </div>
        </article>
      ))}
    </div>
  )
}

function UsersTable({ users, onManage }: UserRowsProps) {
  return (
    <div className="border-border hidden overflow-hidden rounded-md border md:block">
      <Table className="text-left">
        <TableHeader className="bg-surface-muted">
          <TableRow>
            <TableHead className="text-muted-foreground px-3 py-2 font-semibold">Name</TableHead>
            <TableHead className="text-muted-foreground px-3 py-2 font-semibold">Email</TableHead>
            <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
              Global role
            </TableHead>
            <TableHead className="text-muted-foreground px-3 py-2 font-semibold">Team</TableHead>
            <TableHead className="text-muted-foreground px-3 py-2 font-semibold">Status</TableHead>
            <TableHead className="text-muted-foreground px-3 py-2 font-semibold">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {users.map((user) => (
            <TableRow key={user.id}>
              <TableCell className="text-foreground px-3 py-3">
                <div className="flex min-w-0 items-center gap-3">
                  <GeneratedAvatar kind="user" name={user.name || user.email} size={32} />
                  <span className="truncate">{user.name}</span>
                </div>
              </TableCell>
              <TableCell className="text-subtle-foreground px-3 py-3">{user.email}</TableCell>
              <TableCell className="text-subtle-foreground px-3 py-3">{user.global_role}</TableCell>
              <TableCell className="text-subtle-foreground px-3 py-3">
                {user.team_name ?? '—'}
              </TableCell>
              <TableCell className="px-3 py-3">
                <Badge variant={statusBadgeVariant(user.status)}>{user.status}</Badge>
              </TableCell>
              <TableCell className="px-3 py-3">
                <Button type="button" size="sm" variant="secondary" onClick={() => onManage(user)}>
                  Manage
                </Button>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
