import { createFileRoute } from '@tanstack/react-router'

import {
  ApiKeysCard,
  CreateApiKeyDialog,
  CreatedApiKeyAlert,
  ManageApiKeyDialog,
} from '@/routes/api-keys/-components'
import { requireAdminSession } from '@/routes/-admin-guard'
import { getApiKeys } from '@/server/admin-data.functions'
import type { ApiKeysPayload } from '@/types/api'

import { useApiKeysPageState } from './api-keys/-use-api-keys-page'

export const Route = createFileRoute('/api-keys')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  validateSearch: (search: Record<string, unknown>) => ({
    api_key_id: typeof search.api_key_id === 'string' ? search.api_key_id : undefined,
  }),
  loader: () => getApiKeys(),
  component: ApiKeysPage,
})

export function ApiKeysPage() {
  const {
    data: { items, users, service_accounts, models },
  } = Route.useLoaderData() as { data: ApiKeysPayload }
  const search = Route.useSearch()
  const state = useApiKeysPageState({
    items,
    users,
    service_accounts,
    focusedApiKeyId: search.api_key_id,
  })

  return (
    <div className="flex flex-col gap-4">
      <CreatedApiKeyAlert
        result={state.createdResult}
        onCopy={state.actions.handleCopy}
        onDismiss={() => state.actions.setCreatedResult(null)}
      />

      <ApiKeysCard
        items={items}
        onCreate={state.actions.openCreateDialog}
        onManage={state.actions.openManageDialog}
      />

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

      <ManageApiKeyDialog
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
    </div>
  )
}
