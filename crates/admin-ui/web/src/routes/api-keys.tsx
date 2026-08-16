import { createFileRoute } from '@tanstack/react-router'

import {
  ApiKeysCard,
  CreateApiKeyDialog,
  CreatedApiKeyAlert,
  ManageApiKeyDialog,
} from '@/routes/api-keys/-components'
import { PageHeader } from '@/components/layout/page-header'
import { requireAuthenticatedSession } from '@/routes/-admin-guard'
import { canPerformAdminAction } from '@/routes/-auth-routing'
import { getApiKeys } from '@/server/admin-data.functions'
import type { ApiKeysPayload } from '@/types/api'

import { useApiKeysPageState } from './api-keys/-use-api-keys-page'

export const Route = createFileRoute('/api-keys')({
  validateSearch: (search: Record<string, unknown>) => ({
    api_key_id: typeof search.api_key_id === 'string' ? search.api_key_id : undefined,
  }),
  beforeLoad: ({ location }) => requireAuthenticatedSession(location),
  loader: () => getApiKeys(),
  component: ApiKeysPage,
})

export function ApiKeysPage() {
  const {
    data: { items, users, service_accounts, models },
  } = Route.useLoaderData() as { data: ApiKeysPayload }
  const { session } = Route.useRouteContext()
  const canCreate = canPerformAdminAction(session, 'create_api_key')
  const canUpdate = canPerformAdminAction(session, 'update_api_key')
  const canRevoke = canPerformAdminAction(session, 'revoke_api_key')
  const canReveal = canPerformAdminAction(session, 'reveal_api_key')
  const canManage = canUpdate || canRevoke || canReveal
  const search = Route.useSearch()
  const state = useApiKeysPageState({
    items,
    users,
    service_accounts,
    defaultOwnerUserId:
      session.permissions.group === 'platform_admins' ? undefined : session.user.id,
    focusedApiKeyId: canManage ? search.api_key_id : undefined,
  })

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Control Plane"
        title="API keys"
        description={
          canCreate || canManage
            ? 'Create and manage API keys within your access scope.'
            : 'Review the API keys within your access scope.'
        }
      />

      {canCreate ? (
        <CreatedApiKeyAlert
          result={state.createdResult}
          onCopy={state.actions.handleCopy}
          onDismiss={() => state.actions.setCreatedResult(null)}
        />
      ) : null}

      <ApiKeysCard
        items={items}
        onCreate={canCreate ? state.actions.openCreateDialog : undefined}
        onManage={canManage ? state.actions.openManageDialog : undefined}
      />

      {canCreate ? (
        <CreateApiKeyDialog
          form={state.form}
          isPending={state.isPending}
          modelOptions={models}
          open={state.isCreateOpen}
          ownerLabel={state.selectedOwnerLabel}
          serviceAccountOptions={service_accounts}
          userOptions={users}
          submitDisabled={state.isCreateDisabled}
          onModelGrantModeChange={state.actions.updateModelGrantMode}
          onModelToggle={state.actions.toggleModelKey}
          onNameChange={state.actions.updateName}
          onOpenChange={(open) => (!open ? state.actions.closeCreateDialog() : undefined)}
          onOwnerKindChange={state.actions.updateOwnerKind}
          onOwnerSelectionChange={state.actions.updateOwnerSelection}
          onSubmit={state.actions.handleCreateApiKey}
        />
      ) : null}

      {canManage ? (
        <ManageApiKeyDialog
          canReveal={canReveal}
          canRevoke={canRevoke}
          canUpdate={canUpdate}
          form={state.manageForm}
          isPending={state.isPending}
          modelOptions={models}
          open={state.manageDialog.mode === 'open'}
          revealedKey={state.revealedManageKey}
          submitDisabled={state.isManageDisabled}
          target={state.manageTarget}
          onModelGrantModeChange={state.actions.updateManageModelGrantMode}
          onModelToggle={state.actions.toggleManageModelKey}
          onOpenChange={(open) => (!open ? state.actions.closeManageDialog() : undefined)}
          onReveal={state.actions.handleRevealManageApiKey}
          onRevoke={state.actions.handleRevokeApiKey}
          onCopy={state.actions.handleCopy}
          onSubmit={state.actions.handleUpdateApiKey}
        />
      ) : null}
    </div>
  )
}
