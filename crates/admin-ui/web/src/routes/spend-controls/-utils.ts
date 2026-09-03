import type {
  BudgetAlertHistoryItemView,
  BudgetScopeRequest,
  ModelView,
  SpendBudgetServiceAccountView,
  SpendBudgetUserModelView,
  SpendBudgetUserView,
  UpsertBudgetInput,
} from '@/types/api'

export type BudgetSettingsForm = Omit<UpsertBudgetInput, 'scope'>
export type BudgetSettings = NonNullable<SpendBudgetUserView['budget']>
export type BudgetSource = NonNullable<SpendBudgetUserView['budget_source']>

export type BudgetDialogState =
  | { mode: 'closed' }
  | { mode: 'user'; user: SpendBudgetUserView }
  | { mode: 'service_account'; serviceAccount: SpendBudgetServiceAccountView }
  | { mode: 'user_model'; budget: SpendBudgetUserModelView }

export type OpenBudgetDialogState = Exclude<BudgetDialogState, { mode: 'closed' }>

export type UserModelDraft = {
  userId: string
  selectorKind: 'model_id' | 'upstream_model'
  selectorValue: string
  settings: BudgetSettingsForm
}

export const initialBudgetSettings: BudgetSettingsForm = {
  cadence: 'daily',
  amount_usd: '0.0000',
  hard_limit: true,
  timezone: 'UTC',
}

const CURRENCY_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
})

export function formatUsd10000(amountUsd10000: number) {
  return CURRENCY_FORMATTER.format(amountUsd10000 / 10_000)
}

const BUDGET_SOURCE_LABELS: Record<string, string> = {
  manual: 'Manual',
  config_user_override: 'Config (user)',
  config_user_default: 'Config default',
  config_user_model_default: 'Config model default',
  config_service_account: 'Config (service account)',
}

export function budgetSourceLabel(kind: string) {
  return BUDGET_SOURCE_LABELS[kind] ?? kind
}

export function isInheritedBudgetSource(source: BudgetSource | null | undefined) {
  return source != null && source.kind !== 'manual'
}

export const INHERITED_BUDGET_WARNING =
  'This budget is inherited from configuration. Saving converts it to a manual budget that config reloads will not change.'

export const INVALID_BUDGET_AMOUNT_MESSAGE =
  'Amount must be greater than 0 with at most four decimal places'

const BUDGET_AMOUNT_PATTERN = /^(\d+)(?:\.(\d{1,4}))?$/

/**
 * Normalises a user-entered USD amount to the 4-decimal string the API expects.
 * Works on the string itself so no precision is lost and nothing is rounded:
 * the server rejects more than four decimals, so the client does too.
 */
export function normalizeBudgetAmount(value: string): string | null {
  const match = BUDGET_AMOUNT_PATTERN.exec(value.trim())
  if (match === null) {
    return null
  }
  const integer = match[1].replace(/^0+(?=\d)/, '')
  const fraction = (match[2] ?? '').padEnd(4, '0')
  if (/^0*$/.test(integer + fraction)) {
    return null
  }
  return `${integer}.${fraction}`
}

export type BudgetPayloadResult =
  | { ok: true; payload: UpsertBudgetInput }
  | { ok: false; error: string }

export function budgetPayload(
  scope: BudgetScopeRequest,
  settings: BudgetSettingsForm,
): BudgetPayloadResult {
  const amount = normalizeBudgetAmount(settings.amount_usd)
  if (amount === null) {
    return { ok: false, error: INVALID_BUDGET_AMOUNT_MESSAGE }
  }
  return {
    ok: true,
    payload: {
      scope,
      cadence: settings.cadence,
      amount_usd: amount,
      hard_limit: settings.hard_limit,
      timezone: settings.timezone?.trim() || 'UTC',
    },
  }
}

export function createInitialUserModelDraft(
  users: SpendBudgetUserView[],
  models: ModelView[],
): UserModelDraft {
  return {
    userId: users[0]?.user_id ?? '',
    selectorKind: 'model_id',
    selectorValue: models[0]?.model_id ?? '',
    settings: initialBudgetSettings,
  }
}

export function settingsFromBudget(budget?: BudgetSettings | null): BudgetSettingsForm {
  return {
    cadence: budget?.cadence ?? 'daily',
    amount_usd: budget?.amount_usd ?? '0.0000',
    hard_limit: budget?.hard_limit ?? true,
    timezone: budget?.timezone ?? 'UTC',
  }
}

export function scopeForUserModelDraft(draft: UserModelDraft): BudgetScopeRequest | null {
  const selectorValue = draft.selectorValue.trim()
  if (!draft.userId || !selectorValue) {
    return null
  }
  return draft.selectorKind === 'model_id'
    ? { kind: 'user_model', user_id: draft.userId, model_id: selectorValue }
    : { kind: 'user_model', user_id: draft.userId, upstream_model: selectorValue }
}

export function scopeForDialog(dialogState: OpenBudgetDialogState): BudgetScopeRequest {
  if (dialogState.mode === 'user') {
    return { kind: 'user', user_id: dialogState.user.user_id }
  }
  if (dialogState.mode === 'service_account') {
    return {
      kind: 'service_account',
      service_account_id: dialogState.serviceAccount.service_account_id,
    }
  }
  return scopeForUserModelBudget(dialogState.budget)
}

export function budgetSourceForDialog(
  dialogState: OpenBudgetDialogState,
): BudgetSource | null | undefined {
  if (dialogState.mode === 'user') {
    return dialogState.user.budget_source
  }
  if (dialogState.mode === 'service_account') {
    return dialogState.serviceAccount.budget_source
  }
  return dialogState.budget.budget_source
}

export function scopeForUserModelBudget(budget: SpendBudgetUserModelView): BudgetScopeRequest {
  if (budget.model_id) {
    return { kind: 'user_model', user_id: budget.user_id, model_id: budget.model_id }
  }
  return {
    kind: 'user_model',
    user_id: budget.user_id,
    upstream_model: budget.upstream_model ?? '',
  }
}

export function formatUserModelSelector(budget: SpendBudgetUserModelView) {
  if (budget.model_id) {
    return `model:${budget.model_id}`
  }
  return `upstream:${budget.upstream_model ?? ''}`
}

export function getErrorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }
  return 'Request failed'
}

export function badgeVariantForAlert(
  alert: BudgetAlertHistoryItemView,
): 'default' | 'success' | 'warning' {
  if (alert.delivery_status === 'sent') {
    return 'success'
  }
  if (alert.delivery_status === 'pending') {
    return 'warning'
  }
  return 'default'
}

export function formatThreshold(alert: BudgetAlertHistoryItemView) {
  return `${alert.threshold_bps / 100}%`
}

export function formatOwnerKind(ownerKind: string) {
  if (ownerKind === 'service_account') {
    return 'service account'
  }
  return ownerKind
}
