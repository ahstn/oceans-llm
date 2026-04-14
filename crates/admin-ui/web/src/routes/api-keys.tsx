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
  loader: () => getApiKeys(),
  component: ApiKeysPage,
})

export function ApiKeysPage() {
  const {
    data: { items, users, teams, models },
  } = Route.useLoaderData() as { data: ApiKeysPayload }
  const state = useApiKeysPageState({ items, users, teams })

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
        teamOptions={teams}
        userOptions={users}
        submitDisabled={state.isCreateDisabled}
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
        submitDisabled={state.isManageDisabled}
        target={state.manageTarget}
        onModelToggle={state.actions.toggleManageModelKey}
        onOpenChange={(open) => (!open ? state.actions.closeManageDialog() : undefined)}
        onRevoke={state.actions.handleRevokeApiKey}
        onSubmit={state.actions.handleUpdateApiKey}
      />
    </div>
  )
}
