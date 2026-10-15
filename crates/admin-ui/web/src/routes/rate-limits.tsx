import { useState, useTransition } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { Add01Icon, DashboardSpeed01Icon } from '@hugeicons/core-free-icons'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
import { PageHeader } from '@/components/layout/page-header'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Badge } from '@/components/ui/badge'
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import {
  getApiKeys,
  getMcpServers,
  getRateLimitPolicies,
  getTeams,
  removeRateLimitPolicy,
} from '@/server/admin-data.functions'
import type { RateLimitPolicyView } from '@/types/api'

import { PolicyDialog, type PolicyOptions } from './rate-limits/-policy-dialog'
import { describeLimit, describeScope, describeTarget } from './rate-limits/-policy-model'

export const Route = createFileRoute('/rate-limits')({
  loader: async () => {
    const [policies, apiKeys, teams, servers] = await Promise.all([
      getRateLimitPolicies(),
      getApiKeys(),
      getTeams(),
      getMcpServers({ data: { include_disabled: false } }),
    ])
    const options: PolicyOptions = {
      apiKeys: apiKeys.data.items
        .filter((key) => !key.revoked_at)
        .map((key) => ({ value: key.id, label: `${key.name} (${key.prefix})` })),
      users: apiKeys.data.users.map((user) => ({ value: user.id, label: user.email })),
      teams: teams.data.teams.map((team) => ({ value: team.id, label: team.name })),
      models: apiKeys.data.models.map((model) => ({ value: model.key, label: model.key })),
      mcpServers: servers.data.items.map((server) => ({
        value: server.id,
        label: server.server_key,
      })),
    }
    return { policies: policies.data.policies, options }
  },
  component: RateLimitsPage,
})

export function RateLimitsPage() {
  const { policies, options } = Route.useLoaderData()
  const [editing, setEditing] = useState<RateLimitPolicyView | 'new' | null>(null)
  const [deleting, setDeleting] = useState<RateLimitPolicyView | null>(null)

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Control Plane"
        title="Rate Limits"
        description="Cap requests, tokens and MCP tool calls per key, user, team or the whole gateway. Every matching policy applies, and the most restrictive one wins."
        actions={
          <Button size="sm" onClick={() => setEditing('new')}>
            <AppIcon icon={Add01Icon} data-icon="inline-start" aria-hidden />
            New rate limit
          </Button>
        }
      />
      <Card>
        <CardHeader>
          <CardTitle>Policies</CardTitle>
          <CardDescription>
            Policies from gateway.yaml are read-only here. Changes reach every gateway replica
            within the configured refresh interval.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {policies.length === 0 ? (
            <Empty>
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <AppIcon icon={DashboardSpeed01Icon} size={22} stroke={1.5} />
                </EmptyMedia>
                <EmptyTitle>No rate limits</EmptyTitle>
                <EmptyDescription>
                  Requests are limited only by budgets. Add a rate limit to cap traffic.
                </EmptyDescription>
              </EmptyHeader>
              <EmptyContent>
                <Button size="sm" variant="outline" onClick={() => setEditing('new')}>
                  Add the first rate limit
                </Button>
              </EmptyContent>
            </Empty>
          ) : (
            <PolicyList policies={policies} onEdit={setEditing} onDelete={setDeleting} />
          )}
        </CardContent>
      </Card>
      {editing && (
        <PolicyDialog
          key={editing === 'new' ? 'new' : (editing.id ?? editing.name)}
          policy={editing === 'new' ? null : editing}
          options={options}
          onClose={() => setEditing(null)}
        />
      )}
      <DeletePolicyDialog policy={deleting} onClose={() => setDeleting(null)} />
    </div>
  )
}

type PolicyListProps = {
  policies: RateLimitPolicyView[]
  onEdit: (policy: RateLimitPolicyView) => void
  onDelete: (policy: RateLimitPolicyView) => void
}

function PolicyList({ policies, onEdit, onDelete }: PolicyListProps) {
  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-3 md:hidden">
        {policies.map((policy) => (
          <article
            key={policy.id ?? policy.name}
            aria-label={policy.name}
            className="border-border bg-surface-muted rounded-lg border p-4"
          >
            <div className="flex items-start justify-between gap-3">
              <p className="text-foreground min-w-0 truncate font-semibold">{policy.name}</p>
              <PolicyBadges policy={policy} />
            </div>
            <dl className="mt-3 grid gap-2 text-sm">
              <PolicyFact label="Limit">{describeLimit(policy)}</PolicyFact>
              <PolicyFact label="Traffic">{describeTarget(policy)}</PolicyFact>
              <PolicyFact label="Counted per">{describeScope(policy)}</PolicyFact>
            </dl>
            <PolicyActions policy={policy} onEdit={onEdit} onDelete={onDelete} />
          </article>
        ))}
      </div>
      <div className="border-border hidden overflow-hidden rounded-md border md:block">
        <Table className="text-left">
          <TableHeader className="bg-surface-muted">
            <TableRow>
              {['Name', 'Limit', 'Traffic', 'Counted per', 'Status', 'Actions'].map((label) => (
                <TableHead key={label} className="text-muted-foreground px-3 py-2 font-semibold">
                  {label}
                </TableHead>
              ))}
            </TableRow>
          </TableHeader>
          <TableBody>
            {policies.map((policy) => (
              <TableRow key={policy.id ?? policy.name}>
                <TableCell className="px-3 py-3 font-medium">{policy.name}</TableCell>
                <TableCell className="px-3 py-3 tabular-nums">{describeLimit(policy)}</TableCell>
                <TableCell className="px-3 py-3">{describeTarget(policy)}</TableCell>
                <TableCell className="px-3 py-3">{describeScope(policy)}</TableCell>
                <TableCell className="px-3 py-3">
                  <PolicyBadges policy={policy} />
                </TableCell>
                <TableCell className="px-3 py-3">
                  <PolicyActions policy={policy} onEdit={onEdit} onDelete={onDelete} />
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}

function PolicyFact({ label, children }: { label: string; children: string }) {
  return (
    <div>
      <dt className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
        {label}
      </dt>
      <dd className="mt-0.5">{children}</dd>
    </div>
  )
}

function PolicyBadges({ policy }: { policy: RateLimitPolicyView }) {
  return (
    <div className="flex flex-wrap gap-1">
      {policy.source === 'config' && <Badge variant="outline">Config</Badge>}
      <Badge variant={policy.enabled ? 'success' : 'secondary'}>
        {policy.enabled ? 'Enforced' : 'Paused'}
      </Badge>
    </div>
  )
}

function PolicyActions({
  policy,
  onEdit,
  onDelete,
}: {
  policy: RateLimitPolicyView
  onEdit: (policy: RateLimitPolicyView) => void
  onDelete: (policy: RateLimitPolicyView) => void
}) {
  if (policy.source === 'config') {
    return <span className="text-muted-foreground text-xs">Edit in gateway.yaml</span>
  }
  return (
    <div className="mt-3 flex gap-2 md:mt-0">
      <Button variant="outline" size="sm" onClick={() => onEdit(policy)}>
        Edit
      </Button>
      <Button variant="outline" size="sm" onClick={() => onDelete(policy)}>
        Delete
      </Button>
    </div>
  )
}

function DeletePolicyDialog({
  policy,
  onClose,
}: {
  policy: RateLimitPolicyView | null
  onClose: () => void
}) {
  const router = useRouter()
  const [pending, startTransition] = useTransition()

  function confirm() {
    if (!policy?.id) return
    const policyId = policy.id
    startTransition(async () => {
      try {
        await removeRateLimitPolicy({ data: { policyId } })
        await router.invalidate()
        toast.success('Rate limit deleted')
        onClose()
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to delete rate limit')
      }
    })
  }

  return (
    <AlertDialog
      open={Boolean(policy)}
      onOpenChange={(open) => {
        if (!open && !pending) onClose()
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete {policy?.name}?</AlertDialogTitle>
          <AlertDialogDescription>
            Requests stop counting against this limit straight away. Other policies still apply.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={pending}>Cancel</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={confirm} disabled={pending}>
            Delete rate limit
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
