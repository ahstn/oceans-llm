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
import {
  formatLastUsedAt,
  formatModelGrantSummary,
  maskApiKeyPrefix,
} from '@/routes/api-keys/-components'
import type { ApiKeyView } from '@/types/api'

export const COLLAPSED_KEY_COUNT = 3

/** Where the viewer can go to manage or create keys; absent when they lack access. */
export type ApiKeyLinks = {
  canManage: boolean
  canCreate: boolean
}

export function ProfileApiKeysTable({
  keys,
  links,
  dense = false,
}: {
  keys: ApiKeyView[]
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
      <div className="overflow-hidden rounded-md border border-[color:var(--color-border)]">
        <Table>
          <TableHeader className="bg-[color:var(--color-surface-muted)]">
            <TableRow>
              <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                Name
              </TableHead>
              {dense ? null : (
                <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                  Models
                </TableHead>
              )}
              <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                Last used
              </TableHead>
              {dense ? null : (
                <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
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
                    <span className="flex items-center gap-2 font-mono text-xs text-[var(--color-text-soft)]">
                      {maskApiKeyPrefix(key.prefix)}
                      {dense && key.status !== 'active' ? (
                        <KeyStatusBadge status={key.status} />
                      ) : null}
                    </span>
                  </div>
                </TableCell>
                {dense ? null : (
                  <TableCell className="max-w-56 truncate px-3 py-2.5 text-[var(--color-text-soft)]">
                    {formatModelGrantSummary(key)}
                  </TableCell>
                )}
                <TableCell className="px-3 py-2.5 whitespace-nowrap text-[var(--color-text-soft)] tabular-nums">
                  {formatLastUsedAt(key.last_used_at ?? null)}
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
    <Empty className="border border-dashed border-[color:var(--color-border)]">
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
  if (!links.canManage) return null
  return (
    <Button asChild variant="outline" size="sm">
      <Link to="/api-keys">Manage keys</Link>
    </Button>
  )
}

function KeyStatusBadge({ status }: { status: ApiKeyView['status'] }) {
  return <Badge variant={status === 'active' ? 'success' : 'warning'}>{status}</Badge>
}
