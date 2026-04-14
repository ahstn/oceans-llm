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
  model_keys: [],
}

const initialManageForm: UpdateApiKeyInput = {
  model_keys: [],
}

export type ManageDialogState = { mode: 'closed' } | { mode: 'open'; apiKeyId: string }

export function useApiKeysPageState({
  items,
  users,
  teams,
}: Pick<ApiKeysPayload, 'items' | 'users' | 'teams'>) {
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
      : (teams.find((team) => team.id === form.owner_team_id)?.name ?? 'Select a team')

  const manageTarget =
    manageDialog.mode === 'open'
      ? (items.find((item) => item.id === manageDialog.apiKeyId) ?? null)
      : null

  const isCreateDisabled =
    isMutating ||
    form.name.trim().length === 0 ||
    form.model_keys.length === 0 ||
    (form.owner_kind === 'user' ? !form.owner_user_id : !form.owner_team_id)

  const isManageDisabled =
    isMutating ||
    !manageTarget ||
    manageTarget.status !== 'active' ||
    manageForm.model_keys.length === 0 ||
    sameModelSelection(manageTarget.model_keys, manageForm.model_keys)

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
      owner_team_id: ownerKind === 'team' ? current.owner_team_id : null,
    }))
  }

  function updateName(name: string) {
    setForm((current) => ({
      ...current,
      name,
    }))
  }

  function updateOwnerSelection(value: string) {
    setForm((current) => ({
      ...current,
      owner_user_id: current.owner_kind === 'user' ? value : null,
      owner_team_id: current.owner_kind === 'team' ? value : null,
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
            model_keys: manageForm.model_keys,
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
