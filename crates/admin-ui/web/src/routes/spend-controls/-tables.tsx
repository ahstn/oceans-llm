import { Button } from '@/components/ui/button'
import type {
  BudgetScopeRequest,
  SpendBudgetServiceAccountView,
  SpendBudgetUserModelView,
  SpendBudgetUserView,
} from '@/types/api'

import {
  ActionCell,
  BudgetCell,
  BudgetRow,
  BudgetTable,
  IdentityCell,
  MoneyCell,
  TextCell,
} from './-components'
import { formatUserModelSelector, scopeForUserModelBudget } from './-utils'

type TableActions = {
  isPending: boolean
  onRemove: (scope: BudgetScopeRequest, message: string) => void
}

export function UserBudgetsTable({
  users,
  isPending,
  onConfigure,
  onRemove,
}: TableActions & {
  users: SpendBudgetUserView[]
  onConfigure: (user: SpendBudgetUserView) => void
}) {
  return (
    <BudgetTable
      title="User Budgets"
      description="Per-user budget configuration and current window spend."
      columns={['User', 'Budget', 'Current spend', 'Alert recipient', 'Actions']}
      emptyMessage="No users are available."
    >
      {users.map((user) => (
        <BudgetRow key={user.user_id}>
          <IdentityCell primary={user.name} secondary={user.email} />
          <BudgetCell budget={user.budget} source={user.budget_source} />
          <MoneyCell amountUsd10000={user.current_window_spend_usd_10000} />
          <TextCell>{user.alert_recipient_summary}</TextCell>
          <ActionCell>
            <Button type="button" size="sm" variant="secondary" onClick={() => onConfigure(user)}>
              Configure
            </Button>
            {user.budget ? (
              <Button
                type="button"
                size="sm"
                variant="ghost"
                disabled={isPending}
                onClick={() =>
                  onRemove({ kind: 'user', user_id: user.user_id }, 'User budget removed')
                }
              >
                Remove
              </Button>
            ) : null}
          </ActionCell>
        </BudgetRow>
      ))}
    </BudgetTable>
  )
}

export function ServiceAccountBudgetsTable({
  serviceAccounts,
  isPending,
  onConfigure,
  onRemove,
}: TableActions & {
  serviceAccounts: SpendBudgetServiceAccountView[]
  onConfigure: (serviceAccount: SpendBudgetServiceAccountView) => void
}) {
  return (
    <BudgetTable
      title="Service Account Budgets"
      description="Active service-account keys require an active service-account budget."
      columns={['Service account', 'Budget', 'Current spend', 'Alert recipients', 'Actions']}
      emptyMessage="No service accounts are available."
    >
      {serviceAccounts.map((serviceAccount) => (
        <BudgetRow key={serviceAccount.service_account_id}>
          <IdentityCell
            primary={serviceAccount.service_account_name}
            secondary={`${serviceAccount.service_account_key} / ${serviceAccount.team_name}`}
          />
          <BudgetCell budget={serviceAccount.budget} source={serviceAccount.budget_source} />
          <MoneyCell amountUsd10000={serviceAccount.current_window_spend_usd_10000} />
          <TextCell tone={serviceAccount.alert_email_ready ? 'default' : 'danger'}>
            {serviceAccount.alert_recipient_summary}
          </TextCell>
          <ActionCell>
            <Button
              type="button"
              size="sm"
              variant="secondary"
              onClick={() => onConfigure(serviceAccount)}
            >
              Configure
            </Button>
            {serviceAccount.budget ? (
              <Button
                type="button"
                size="sm"
                variant="ghost"
                disabled={isPending}
                onClick={() =>
                  onRemove(
                    {
                      kind: 'service_account',
                      service_account_id: serviceAccount.service_account_id,
                    },
                    'Service account budget removed',
                  )
                }
              >
                Remove
              </Button>
            ) : null}
          </ActionCell>
        </BudgetRow>
      ))}
    </BudgetTable>
  )
}

export function UserModelBudgetsTable({
  budgets,
  usersById,
  isPending,
  onConfigure,
  onRemove,
}: TableActions & {
  budgets: SpendBudgetUserModelView[]
  usersById: Map<string, SpendBudgetUserView>
  onConfigure: (budget: SpendBudgetUserModelView) => void
}) {
  return (
    <BudgetTable
      title="User Model Budgets"
      description="Model-specific budgets are evaluated before the user's general budget."
      columns={['User', 'Model scope', 'Budget', 'Current spend', 'Actions']}
      emptyMessage="No user model budgets are configured."
    >
      {budgets.map((budget) => {
        const user = usersById.get(budget.user_id)
        return (
          <BudgetRow key={budget.scope_key}>
            <IdentityCell
              primary={user?.name ?? budget.user_id}
              secondary={user?.email ?? budget.user_id}
            />
            <TextCell>{formatUserModelSelector(budget)}</TextCell>
            <BudgetCell budget={budget.budget} source={budget.budget_source} />
            <MoneyCell amountUsd10000={budget.current_window_spend_usd_10000} />
            <ActionCell>
              <Button
                type="button"
                size="sm"
                variant="secondary"
                onClick={() => onConfigure(budget)}
              >
                Configure
              </Button>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                disabled={isPending}
                onClick={() =>
                  onRemove(scopeForUserModelBudget(budget), 'User model budget removed')
                }
              >
                Remove
              </Button>
            </ActionCell>
          </BudgetRow>
        )
      })}
    </BudgetTable>
  )
}
