import { Link, createFileRoute } from '@tanstack/react-router'
import { RoboticIcon, SearchIcon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { canAccessPage } from '@/components/layout/admin-nav'
import { PageHeader } from '@/components/layout/page-header'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from '@/components/ui/empty'
import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import { EntityTagBadges } from '@/routes/identity/-entity-tags'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { getApiKeys, getServiceAccounts } from '@/server/admin-data.functions'
import type { ApiKeyView, ServiceAccountView } from '@/types/api'

export const Route = createFileRoute('/identity/service-accounts')({
  loader: async ({ context }) => {
    const [serviceAccounts, apiKeys] = await Promise.all([
      getServiceAccounts(),
      context.session?.permissions.group === 'users' ? null : getApiKeys(),
    ])

    return {
      serviceAccounts: serviceAccounts.data.service_accounts,
      apiKeys: apiKeys?.data.items ?? [],
    }
  },
  component: ServiceAccountsPage,
})

type ServiceAccountCredentialRow = {
  id: string
  serviceAccount: ServiceAccountView
  apiKey: ApiKeyView | null
}

export function ServiceAccountsPage() {
  const { session } = Route.useRouteContext()
  const { serviceAccounts, apiKeys } = Route.useLoaderData() as {
    serviceAccounts: ServiceAccountView[]
    apiKeys: ApiKeyView[]
  }
  const rows = buildServiceAccountRows(serviceAccounts, apiKeys)
  const credentialAccessRestricted = session?.permissions.group === 'users'

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Identity"
        title="Service accounts"
        description="Review automated accounts, their teams, and the API keys that they use."
      />

      <Card>
        <CardHeader>
          <div className="flex flex-col gap-1">
            <CardTitle>Account list</CardTitle>
            <CardDescription>
              Open a team or API key from the table. You cannot make changes on this page.
            </CardDescription>
          </div>
        </CardHeader>
        <CardContent>
          {serviceAccounts.length === 0 ? (
            <Empty>
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <AppIcon icon={RoboticIcon} size={22} stroke={1.5} />
                </EmptyMedia>
                <EmptyTitle>No service accounts visible</EmptyTitle>
                <EmptyDescription>
                  No service accounts are visible for the current scope. Create or grant access to
                  service accounts in the gateway configuration, then return here to audit ownership
                  and credentials.
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <ServiceAccountTable
              rows={rows}
              credentialAccessRestricted={credentialAccessRestricted}
            />
          )}
        </CardContent>
      </Card>
    </div>
  )
}

function ServiceAccountTable({
  rows,
  credentialAccessRestricted,
}: {
  rows: ServiceAccountCredentialRow[]
  credentialAccessRestricted: boolean
}) {
  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-3 md:hidden">
        {rows.map((row) => (
          <article
            key={row.id}
            className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4"
          >
            <div className="flex items-start justify-between gap-3">
              <div className="flex min-w-0 items-center gap-3">
                <AppIcon icon={RoboticIcon} size={24} stroke={1.5} className="shrink-0" />
                <div className="min-w-0">
                  <p className="truncate font-semibold text-[var(--color-text)]">
                    {row.serviceAccount.name}
                  </p>
                  <p className="truncate font-mono text-xs text-[var(--color-text-soft)]">
                    {row.serviceAccount.key}
                  </p>
                </div>
              </div>
              <StatusBadge status={row.serviceAccount.status} />
            </div>

            <dl className="mt-4 grid gap-3 text-sm">
              <div>
                <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                  Team
                </dt>
                <dd className="mt-1">
                  <TeamLink serviceAccount={row.serviceAccount} />
                </dd>
              </div>
              <div>
                <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                  Tags
                </dt>
                <dd className="mt-1">
                  <EntityTagBadges tags={row.serviceAccount.tags} />
                </dd>
              </div>
              <div>
                <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                  API key
                </dt>
                <dd className="mt-1 text-[var(--color-text-muted)]">
                  {credentialLabel(row.apiKey, credentialAccessRestricted)}
                </dd>
              </div>
            </dl>

            <div className="mt-4 flex flex-wrap gap-2">
              {row.apiKey ? <ApiKeyLink apiKey={row.apiKey} /> : null}
            </div>
          </article>
        ))}
      </div>

      <div className="hidden overflow-hidden rounded-md border border-[color:var(--color-border)] md:block">
        <Table className="text-left">
          <TableHeader className="bg-[color:var(--color-surface-muted)]">
            <TableRow>
              <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                Service account name
              </TableHead>
              <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                Service account key
              </TableHead>
              <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                Team
              </TableHead>
              <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                Status
              </TableHead>
              <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                Tags
              </TableHead>
              <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                API key name
              </TableHead>
              <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                Actions
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => (
              <TableRow key={row.id}>
                <TableCell className="px-3 py-3 text-[var(--color-text)]">
                  <div className="flex min-w-0 items-center gap-3">
                    <AppIcon icon={RoboticIcon} size={22} stroke={1.5} className="shrink-0" />
                    <span className="truncate font-semibold">{row.serviceAccount.name}</span>
                  </div>
                </TableCell>
                <TableCell className="px-3 py-3 font-mono text-xs text-[var(--color-text-muted)]">
                  {row.serviceAccount.key}
                </TableCell>
                <TableCell className="px-3 py-3">
                  <TeamLink serviceAccount={row.serviceAccount} compact />
                </TableCell>
                <TableCell className="px-3 py-3">
                  <StatusBadge status={row.serviceAccount.status} />
                </TableCell>
                <TableCell className="px-3 py-3">
                  <EntityTagBadges tags={row.serviceAccount.tags} />
                </TableCell>
                <TableCell className="px-3 py-3 text-[var(--color-text-muted)]">
                  {credentialLabel(row.apiKey, credentialAccessRestricted)}
                </TableCell>
                <TableCell className="px-3 py-3">
                  {row.apiKey ? (
                    <ApiKeyLink apiKey={row.apiKey} />
                  ) : (
                    <span className="text-xs text-[var(--color-text-soft)]">No API key</span>
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}

function TeamLink({
  serviceAccount,
  compact = false,
}: {
  serviceAccount: ServiceAccountView
  compact?: boolean
}) {
  const { session } = Route.useRouteContext()
  if (!session || !canAccessPage(session, 'teams')) {
    return (
      <span className="inline-flex items-center gap-2">
        <GeneratedAvatar kind="team" name={serviceAccount.team_name} size={compact ? 20 : 24} />
        <span className="truncate">{serviceAccount.team_name}</span>
      </span>
    )
  }

  return (
    <Button asChild type="button" size="sm" variant="secondary" className="h-auto px-2 py-1">
      <Link to="/identity/teams" aria-label={`Open ${serviceAccount.team_name} in Teams`}>
        <GeneratedAvatar kind="team" name={serviceAccount.team_name} size={compact ? 20 : 24} />
        <span className="truncate">{serviceAccount.team_name}</span>
        {!compact ? (
          <span className="font-mono text-xs text-[var(--color-text-soft)]">
            {serviceAccount.team_key}
          </span>
        ) : null}
      </Link>
    </Button>
  )
}

function ApiKeyLink({ apiKey }: { apiKey: ApiKeyView }) {
  const { session } = Route.useRouteContext()
  if (!session || !canAccessPage(session, 'api_keys')) return null

  return (
    <Button asChild type="button" size="sm" variant="secondary">
      <Link
        to="/api-keys"
        search={{ api_key_id: apiKey.id }}
        aria-label={`Open API key ${apiKey.name}`}
      >
        <AppIcon icon={SearchIcon} size={16} stroke={1.5} data-icon="inline-start" />
        Open API key
      </Link>
    </Button>
  )
}

function credentialLabel(apiKey: ApiKeyView | null, accessRestricted: boolean) {
  if (apiKey) return apiKey.name
  return accessRestricted ? 'Credential details restricted' : 'No credential attached'
}

function StatusBadge({ status }: { status: string }) {
  return <Badge variant={status === 'active' ? 'success' : 'warning'}>{status}</Badge>
}

function buildServiceAccountRows(
  serviceAccounts: ServiceAccountView[],
  apiKeys: ApiKeyView[],
): ServiceAccountCredentialRow[] {
  return serviceAccounts.flatMap((serviceAccount) => {
    const attachedApiKeys = apiKeys.filter(
      (apiKey) => apiKey.owner_kind === 'service_account' && apiKey.owner_id === serviceAccount.id,
    )

    if (attachedApiKeys.length === 0) {
      return [{ id: serviceAccount.id, serviceAccount, apiKey: null }]
    }

    return attachedApiKeys.map((apiKey) => ({
      id: `${serviceAccount.id}:${apiKey.id}`,
      serviceAccount,
      apiKey,
    }))
  })
}
