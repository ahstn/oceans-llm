import { useState } from 'react'
import { Key01Icon } from '@hugeicons/core-free-icons'
import { Link } from '@tanstack/react-router'

import { AppIcon } from '@/components/icons/app-icon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { formatLastUsedAt, formatModelGrantSummary, maskApiKeyPrefix } from '@/lib/api-key-format'
import type { MyProfileApiKeyView } from '@/types/api'

export const COLLAPSED_KEY_COUNT = 3

/** Where the viewer can go to manage or create keys; absent when they lack access. */
export type ApiKeyLinks = {
  /** Can open the API keys page. */
  canView: boolean
  /** Can open a key's manage dialog, so per-key links lead somewhere. */
  canManage: boolean
  canCreate: boolean
}

export function ProfileApiKeysTable({
  keys,
  links,
  dense = false,
}: {
  keys: MyProfileApiKeyView[]
  links: ApiKeyLinks
  /** Drops the models column and inlines status for narrow layouts. */
  dense?: boolean
}) {
  const [expanded, setExpanded] = useState(false)

  if (keys.length === 0) {
    return <ProfileApiKeysEmpty canCreate={links.canCreate} />
  }

  const visible = expanded ? keys : keys.slice(0, COLLAPSED_KEY_COUNT)
  const hidden = keys.length - COLLAPSED_KEY_COUNT

  return (
    <div className="flex flex-col gap-3">
      <div className="border-border overflow-hidden rounded-md border">
        <Table>
          <TableHeader className="bg-surface-muted">
            <TableRow>
              <TableHead className="text-muted-foreground px-3 py-2 font-semibold">Name</TableHead>
              {dense ? null : (
                <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
                  Models
                </TableHead>
              )}
              <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
                Last used
              </TableHead>
              {dense ? null : (
                <TableHead className="text-muted-foreground px-3 py-2 font-semibold">
                  Status
                </TableHead>
              )}
            </TableRow>
          </TableHeader>
          <TableBody>
            {visible.map((key) => (
              <TableRow key={key.id}>
                <TableCell className="px-3 py-2.5">
                  <div className="flex min-w-0 flex-col gap-0.5">
                    {links.canManage ? (
                      <Link
                        to="/api-keys"
                        search={{ api_key_id: key.id }}
                        className="truncate font-medium hover:underline"
                      >
                        {key.name}
                      </Link>
                    ) : (
                      <span className="truncate font-medium">{key.name}</span>
                    )}
                    <span className="text-muted-foreground flex items-center gap-2 font-mono text-xs">
                      {maskApiKeyPrefix(key.prefix)}
                      {dense && key.status !== 'active' ? (
                        <KeyStatusBadge status={key.status} />
                      ) : null}
                    </span>
                  </div>
                </TableCell>
                {dense ? null : (
                  <TableCell className="text-muted-foreground max-w-56 truncate px-3 py-2.5">
                    {formatModelGrantSummary(key)}
                  </TableCell>
                )}
                <TableCell className="text-muted-foreground px-3 py-2.5 whitespace-nowrap tabular-nums">
                  {formatLastUsedAt(key.last_used_at)}
                </TableCell>
                {dense ? null : (
                  <TableCell className="px-3 py-2.5">
                    <KeyStatusBadge status={key.status} />
                  </TableCell>
                )}
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
      {hidden > 0 ? (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="self-start"
          aria-expanded={expanded}
          onClick={() => setExpanded((current) => !current)}
        >
          {expanded ? 'Show fewer' : `Show ${hidden} more`}
        </Button>
      ) : null}
    </div>
  )
}

function ProfileApiKeysEmpty({ canCreate }: { canCreate: boolean }) {
  return (
    <Empty className="border-border border border-dashed">
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <AppIcon icon={Key01Icon} size={22} stroke={1.5} />
        </EmptyMedia>
        <EmptyTitle>No personal API keys yet</EmptyTitle>
        <EmptyDescription>
          {canCreate
            ? 'Create a key to connect a client or agent harness to the gateway. Usage from the key appears on this page.'
            : 'No gateway keys are assigned to you. Ask a platform admin to create one.'}
        </EmptyDescription>
      </EmptyHeader>
      {canCreate ? (
        <EmptyContent>
          <Button asChild>
            <Link to="/api-keys" search={{ create: true }}>
              Create your first key
            </Link>
          </Button>
        </EmptyContent>
      ) : null}
    </Empty>
  )
}

/** Header action linking to the full API keys page. */
export function ManageKeysLink({ links }: { links: ApiKeyLinks }) {
  if (!links.canView) return null
  return (
    <Button asChild variant="outline" size="sm">
      <Link to="/api-keys">{links.canManage || links.canCreate ? 'Manage keys' : 'View keys'}</Link>
    </Button>
  )
}

function KeyStatusBadge({ status }: { status: MyProfileApiKeyView['status'] }) {
  return <Badge variant={status === 'active' ? 'success' : 'warning'}>{status}</Badge>
}
