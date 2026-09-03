import { useMemo, useState, useTransition, type FormEvent } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { PageHeader } from '@/components/layout/page-header'
import {
  getBudgetAlertHistory,
  getModels,
  getSpendBudgets,
  removeBudget,
  saveBudget,
} from '@/server/admin-data.functions'
import type {
  BudgetAlertHistoryView,
  BudgetScopeRequest,
  ModelView,
  SpendBudgetServiceAccountView,
  SpendBudgetUserModelView,
  SpendBudgetUserView,
  SpendBudgetsView,
} from '@/types/api'

import { AlertHistoryCard, BudgetDialog } from './spend-controls/-components'
import {
  ServiceAccountBudgetsTable,
  UserBudgetsTable,
  UserModelBudgetsTable,
} from './spend-controls/-tables'
import { UserModelBudgetForm } from './spend-controls/-user-model-form'
import {
  budgetPayload,
  budgetSourceForDialog,
  createInitialUserModelDraft,
  formatUserModelSelector,
  getErrorMessage,
  initialBudgetSettings,
  scopeForDialog,
  scopeForUserModelDraft,
  settingsFromBudget,
  type BudgetDialogState,
  type BudgetSettingsForm,
  type UserModelDraft,
} from './spend-controls/-utils'

export const Route = createFileRoute('/spend-controls')({
  loader: async () => {
    const [budgets, alerts, models] = await Promise.all([
      getSpendBudgets(),
      getBudgetAlertHistory({
        data: { page: 1, page_size: 10, owner_kind: 'all', status: 'all', channel: 'all' },
      }),
      getModels({ data: { page: 1, page_size: 200 } }),
    ])
    return { budgets, alerts, models }
  },
  component: SpendControlsPage,
})

// Budget lists and editor dialogs share scope resolution, drafts, and mutation transitions.
// oxlint-disable-next-line eslint/max-lines-per-function
export function SpendControlsPage() {
  const router = useRouter()
  const {
    budgets: {
      data: { users, service_accounts: serviceAccounts, user_model_budgets: userModelBudgets },
    },
    alerts: {
      data: { items: alertItems },
    },
    models: {
      data: { items: models },
    },
  } = Route.useLoaderData() as {
    budgets: { data: SpendBudgetsView }
    alerts: { data: BudgetAlertHistoryView }
    models: { data: { items: ModelView[] } }
  }
  const [dialogState, setDialogState] = useState<BudgetDialogState>({ mode: 'closed' })
  const [form, setForm] = useState<BudgetSettingsForm>(initialBudgetSettings)
  const [userModelDraft, setUserModelDraft] = useState<UserModelDraft>(() =>
    createInitialUserModelDraft(users, models),
  )
  const [isPending, startTransition] = useTransition()

  const usersById = useMemo(() => new Map(users.map((user) => [user.user_id, user])), [users])
  const openLabel = useMemo(() => {
    if (dialogState.mode === 'user') {
      return dialogState.user.name
    }
    if (dialogState.mode === 'service_account') {
      return dialogState.serviceAccount.service_account_name
    }
    if (dialogState.mode === 'user_model') {
      const userName = usersById.get(dialogState.budget.user_id)?.name ?? dialogState.budget.user_id
      return `${userName} / ${formatUserModelSelector(dialogState.budget)}`
    }
    return null
  }, [dialogState, usersById])

  function openUserDialog(user: SpendBudgetUserView) {
    setDialogState({ mode: 'user', user })
    setForm(settingsFromBudget(user.budget))
  }

  function openServiceAccountDialog(serviceAccount: SpendBudgetServiceAccountView) {
    setDialogState({ mode: 'service_account', serviceAccount })
    setForm(settingsFromBudget(serviceAccount.budget))
  }

  function openUserModelDialog(budget: SpendBudgetUserModelView) {
    setDialogState({ mode: 'user_model', budget })
    setForm(settingsFromBudget(budget.budget))
  }

  function closeDialog() {
    setDialogState({ mode: 'closed' })
    setForm(initialBudgetSettings)
  }

  function handleSave(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (dialogState.mode === 'closed') {
      return
    }

    const result = budgetPayload(scopeForDialog(dialogState), form)
    if (!result.ok) {
      toast.error(result.error)
      return
    }
    startTransition(async () => {
      try {
        await saveBudget({ data: result.payload })
        toast.success('Budget updated')
        await router.invalidate()
        closeDialog()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  function handleCreateUserModelBudget(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const scope = scopeForUserModelDraft(userModelDraft)
    if (scope === null) {
      toast.error('Select a user and model scope before saving')
      return
    }
    const result = budgetPayload(scope, userModelDraft.settings)
    if (!result.ok) {
      toast.error(result.error)
      return
    }

    startTransition(async () => {
      try {
        await saveBudget({ data: result.payload })
        toast.success('User model budget created')
        await router.invalidate()
        setUserModelDraft(createInitialUserModelDraft(users, models))
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  function handleDeactivate(scope: BudgetScopeRequest, message: string) {
    startTransition(async () => {
      try {
        await removeBudget({ data: { scope } })
        toast.success(message)
        await router.invalidate()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Budget & Spending"
        title="Spend controls"
        description="Set spending limits for users, automated accounts, and each user's model use. Review recent alerts."
      />

      <UserBudgetsTable
        users={users}
        isPending={isPending}
        onConfigure={openUserDialog}
        onRemove={handleDeactivate}
      />
      <ServiceAccountBudgetsTable
        serviceAccounts={serviceAccounts}
        isPending={isPending}
        onConfigure={openServiceAccountDialog}
        onRemove={handleDeactivate}
      />
      <UserModelBudgetsTable
        budgets={userModelBudgets}
        usersById={usersById}
        isPending={isPending}
        onConfigure={openUserModelDialog}
        onRemove={handleDeactivate}
      />

      <UserModelBudgetForm
        users={users}
        models={models}
        draft={userModelDraft}
        setDraft={setUserModelDraft}
        isPending={isPending}
        onSubmit={handleCreateUserModelBudget}
      />

      <AlertHistoryCard alerts={alertItems} />

      <BudgetDialog
        open={dialogState.mode !== 'closed'}
        label={openLabel}
        source={dialogState.mode === 'closed' ? null : budgetSourceForDialog(dialogState)}
        form={form}
        setForm={setForm}
        isPending={isPending}
        onClose={closeDialog}
        onSubmit={handleSave}
      />
    </div>
  )
}
