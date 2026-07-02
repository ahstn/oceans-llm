import { useState, type FormEvent } from 'react'
import { useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import {
  createGatewayApiKey,
  revokeGatewayApiKey,
  updateGatewayApiKey,
} from '@/server/admin-data.functions'
import type {
  ApiKeysPayload,
  CreateApiKeyInput,
  CreateApiKeyResult,
  UpdateApiKeyInput,
} from '@/types/api'

const initialForm: CreateApiKeyInput = {
  name: '',
  owner_kind: 'user',
  owner_user_id: null,
  owner_team_id: null,
  owner_service_account_id: null,
  model_grant_mode: 'all',
  model_keys: [],
}

const initialManageForm: UpdateApiKeyInput = {
  model_grant_mode: 'explicit',
  model_keys: [],
}

export type ManageDialogState = { mode: 'closed' } | { mode: 'open'; apiKeyId: string }

export function useApiKeysPageState({
  items,
  users,
  service_accounts,
}: Pick<ApiKeysPayload, 'items' | 'users' | 'service_accounts'>) {
  const router = useRouter()
  const [isCreateOpen, setIsCreateOpen] = useState(false)
  const [form, setForm] = useState<CreateApiKeyInput>(initialForm)
  const [manageForm, setManageForm] = useState<UpdateApiKeyInput>(initialManageForm)
  const [createdResult, setCreatedResult] = useState<CreateApiKeyResult | null>(null)
  const [manageDialog, setManageDialog] = useState<ManageDialogState>({ mode: 'closed' })
  const [isMutating, setIsMutating] = useState(false)

  const selectedOwnerLabel =
    form.owner_kind === 'user'
      ? (users.find((user) => user.id === form.owner_user_id)?.name ?? 'Select a user')
      : (service_accounts.find((account) => account.id === form.owner_service_account_id)?.name ??
        'Select a service account')

  const manageTarget =
    manageDialog.mode === 'open'
      ? (items.find((item) => item.id === manageDialog.apiKeyId) ?? null)
      : null

  const isCreateDisabled =
    isMutating ||
    form.name.trim().length === 0 ||
    (form.model_grant_mode === 'explicit' && form.model_keys.length === 0) ||
    (form.owner_kind === 'user' ? !form.owner_user_id : !form.owner_service_account_id)

  const isManageDisabled =
    isMutating ||
    !manageTarget ||
    manageTarget.status !== 'active' ||
    (manageForm.model_grant_mode === 'explicit' && manageForm.model_keys.length === 0) ||
    (manageTarget.model_grant_mode === manageForm.model_grant_mode &&
      sameModelSelection(manageTarget.model_keys, manageForm.model_keys))

  async function refreshApiKeys() {
    await router.invalidate()
  }

  function openCreateDialog() {
    setForm(initialForm)
    setIsCreateOpen(true)
  }

  function closeCreateDialog() {
    setForm(initialForm)
    setIsCreateOpen(false)
  }

  function openManageDialog(apiKeyId: string) {
    const target = items.find((item) => item.id === apiKeyId)
    setManageForm({
      model_grant_mode: target?.model_grant_mode ?? 'explicit',
      model_keys: target?.model_keys ?? [],
    })
    setManageDialog({ mode: 'open', apiKeyId })
  }

  function closeManageDialog() {
    setManageForm(initialManageForm)
    setManageDialog({ mode: 'closed' })
  }

  function updateOwnerKind(ownerKind: CreateApiKeyInput['owner_kind']) {
    setForm((current) => ({
      ...current,
      owner_kind: ownerKind,
      owner_user_id: ownerKind === 'user' ? current.owner_user_id : null,
      model_grant_mode: ownerKind === 'user' ? current.model_grant_mode : 'explicit',
      owner_team_id:
        ownerKind === 'service_account'
          ? (service_accounts.find((account) => account.id === current.owner_service_account_id)
              ?.team_id ?? null)
          : null,
      owner_service_account_id:
        ownerKind === 'service_account' ? current.owner_service_account_id : null,
    }))
  }

  function updateModelGrantMode(modelGrantMode: CreateApiKeyInput['model_grant_mode']) {
    setForm((current) => ({
      ...current,
      model_grant_mode: current.owner_kind === 'service_account' ? 'explicit' : modelGrantMode,
      model_keys: modelGrantMode === 'all' ? [] : current.model_keys,
    }))
  }

  function updateManageModelGrantMode(modelGrantMode: UpdateApiKeyInput['model_grant_mode']) {
    if (manageTarget?.owner_kind === 'service_account') {
      return
    }

    setManageForm((current) => ({
      ...current,
      model_grant_mode: modelGrantMode,
      model_keys: modelGrantMode === 'all' ? [] : current.model_keys,
    }))
  }

  function updateName(name: string) {
    setForm((current) => ({
      ...current,
      name,
    }))
  }

  function updateOwnerSelection(value: string) {
    const serviceAccount = service_accounts.find((account) => account.id === value)
    setForm((current) => ({
      ...current,
      owner_user_id: current.owner_kind === 'user' ? value : null,
      owner_team_id:
        current.owner_kind === 'service_account' ? (serviceAccount?.team_id ?? null) : null,
      owner_service_account_id: current.owner_kind === 'service_account' ? value : null,
    }))
  }

  function toggleModelKey(modelKey: string, checked: boolean) {
    setForm((current) => ({
      ...current,
      model_keys: checked
        ? [...current.model_keys, modelKey]
        : current.model_keys.filter((existing) => existing !== modelKey),
    }))
  }

  function toggleManageModelKey(modelKey: string, checked: boolean) {
    setManageForm((current) => ({
      ...current,
      model_keys: checked
        ? [...current.model_keys, modelKey]
        : current.model_keys.filter((existing) => existing !== modelKey),
    }))
  }

  async function handleCreateApiKey(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()

    setIsMutating(true)
    try {
      const response = await createGatewayApiKey({
        data: {
          ...form,
          name: form.name.trim(),
          model_keys: form.model_grant_mode === 'all' ? [] : form.model_keys,
        },
      })
      setCreatedResult(response.data)
      toast.success('API key created')
      await refreshApiKeys()
      closeCreateDialog()
    } catch (error) {
      toast.error(getErrorMessage(error))
    } finally {
      setIsMutating(false)
    }
  }

  async function handleUpdateApiKey(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (manageDialog.mode !== 'open' || !manageTarget || manageTarget.status !== 'active') {
      return
    }

    setIsMutating(true)
    try {
      await updateGatewayApiKey({
        data: {
          apiKeyId: manageDialog.apiKeyId,
          input: {
            model_grant_mode: manageForm.model_grant_mode,
            model_keys: manageForm.model_grant_mode === 'all' ? [] : manageForm.model_keys,
          },
        },
      })
      toast.success('API key updated')
      await refreshApiKeys()
      closeManageDialog()
    } catch (error) {
      toast.error(getErrorMessage(error))
    } finally {
      setIsMutating(false)
    }
  }

  async function handleRevokeApiKey() {
    if (manageDialog.mode !== 'open') {
      return
    }

    setIsMutating(true)
    try {
      await revokeGatewayApiKey({
        data: { apiKeyId: manageDialog.apiKeyId },
      })
      toast.success('API key revoked')
      await refreshApiKeys()
      closeManageDialog()
    } catch (error) {
      toast.error(getErrorMessage(error))
    } finally {
      setIsMutating(false)
    }
  }

  async function handleCopy(value: string, successMessage: string) {
    try {
      await navigator.clipboard.writeText(value)
      toast.success(successMessage)
    } catch {
      toast.error('Clipboard access failed')
    }
  }

  return {
    createdResult,
    form,
    isCreateDisabled,
    isCreateOpen,
    isManageDisabled,
    isPending: isMutating,
    manageDialog,
    manageForm,
    manageTarget,
    selectedOwnerLabel,
    actions: {
      closeCreateDialog,
      closeManageDialog,
      handleCopy,
      handleCreateApiKey,
      handleRevokeApiKey,
      handleUpdateApiKey,
      openCreateDialog,
      openManageDialog,
      setCreatedResult,
      toggleManageModelKey,
      toggleModelKey,
      updateManageModelGrantMode,
      updateModelGrantMode,
      updateName,
      updateOwnerKind,
      updateOwnerSelection,
    },
  }
}

export function getErrorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }
  return 'Request failed'
}

function sameModelSelection(current: string[], next: string[]) {
  if (current.length !== next.length) {
    return false
  }

  const left = [...current].sort()
  const right = [...next].sort()
  return left.every((value, index) => value === right[index])
}
