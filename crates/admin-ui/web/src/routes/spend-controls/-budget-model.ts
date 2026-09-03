import { getBudgetAlertHistory, getModels, getSpendBudgets } from '@/server/admin-data.functions'
import type {
  BudgetAlertHistoryView,
  BudgetScopeRequest,
  ModelView,
  SpendBudgetServiceAccountView,
  SpendBudgetUserModelView,
  SpendBudgetUserView,
  SpendBudgetsView,
  UpsertBudgetInput,
} from '@/types/api'

// Budget domain model: API shapes, source labels, amount validation, scopes, and the
// route loader. Presentation lives in the sibling modules.

export type BudgetSettings = NonNullable<SpendBudgetUserView['budget']>
export type BudgetSource = NonNullable<SpendBudgetUserView['budget_source']>
export type BudgetSettingsForm = Omit<UpsertBudgetInput, 'scope'>

export type SpendControlsLoaderData = {
  budgets: { data: SpendBudgetsView }
  alerts: { data: BudgetAlertHistoryView }
  models: { data: { items: ModelView[] } }
}

export async function loadSpendControls(): Promise<SpendControlsLoaderData> {
  const [budgets, alerts, models] = await Promise.all([
    getSpendBudgets(),
    getBudgetAlertHistory({
      data: {
        page: 1,
        page_size: 10,
        owner_kind: 'all',
        status: 'all',
        channel: 'all',
      },
    }),
    getModels({ data: { page: 1, page_size: 200 } }),
  ])
  return { budgets, alerts, models } as SpendControlsLoaderData
}

// ---------------------------------------------------------------------------
// Budget source
// ---------------------------------------------------------------------------

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

export function isInheritedBudgetSource(
  source: BudgetSource | null | undefined,
): source is BudgetSource {
  return source != null && source.kind !== 'manual'
}

export const INHERITED_BUDGET_WARNING =
  'This budget is inherited from configuration. Saving converts it to a manual budget that config reloads will not change.'

// ---------------------------------------------------------------------------
// Settings form and payload
// ---------------------------------------------------------------------------

export const initialBudgetSettings: BudgetSettingsForm = {
  cadence: 'daily',
  amount_usd: '100.0000',
  hard_limit: true,
  timezone: 'UTC',
}

export function settingsFromBudget(budget?: BudgetSettings | null): BudgetSettingsForm {
  return {
    cadence: budget?.cadence ?? 'daily',
    amount_usd: budget?.amount_usd ?? '0.0000',
    hard_limit: budget?.hard_limit ?? true,
    timezone: budget?.timezone ?? 'UTC',
  }
}

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

// ---------------------------------------------------------------------------
// Scopes and editor targets
// ---------------------------------------------------------------------------

/** Everything the editor dialog needs about the budget being edited, resolved once on open. */
export type BudgetTarget = {
  scope: BudgetScopeRequest
  /** Display text for the dialog, e.g. "Diego" or "Diego / model:claude-sonnet". */
  label: string
  source: BudgetSource | null | undefined
  budget: BudgetSettings | null | undefined
}

export function userScope(user: SpendBudgetUserView): BudgetScopeRequest {
  return { kind: 'user', user_id: user.user_id }
}

export function serviceAccountScope(
  serviceAccount: SpendBudgetServiceAccountView,
): BudgetScopeRequest {
  return {
    kind: 'service_account',
    service_account_id: serviceAccount.service_account_id,
  }
}

export function userModelScope(budget: SpendBudgetUserModelView): BudgetScopeRequest {
  if (budget.model_id) {
    return {
      kind: 'user_model',
      user_id: budget.user_id,
      model_id: budget.model_id,
    }
  }
  return {
    kind: 'user_model',
    user_id: budget.user_id,
    upstream_model: budget.upstream_model ?? '',
  }
}

export function userTarget(user: SpendBudgetUserView): BudgetTarget {
  return {
    scope: userScope(user),
    label: user.name,
    source: user.budget_source,
    budget: user.budget,
  }
}

export function serviceAccountTarget(serviceAccount: SpendBudgetServiceAccountView): BudgetTarget {
  return {
    scope: serviceAccountScope(serviceAccount),
    label: serviceAccount.service_account_name,
    source: serviceAccount.budget_source,
    budget: serviceAccount.budget,
  }
}

export function userModelTarget(budget: SpendBudgetUserModelView, label: string): BudgetTarget {
  return {
    scope: userModelScope(budget),
    label,
    source: budget.budget_source,
    budget: budget.budget,
  }
}

// ---------------------------------------------------------------------------
// Labels
// ---------------------------------------------------------------------------

/**
 * Human label for a model budget scope. `ModelView.model_id` is the UUID the budget
 * stores; `ModelView.id` is the model key shown in the UI. Falls back to the raw id.
 */
export function formatUserModelSelector(
  budget: SpendBudgetUserModelView,
  models: readonly ModelView[] = [],
) {
  if (budget.model_id) {
    const key = models.find((model) => model.model_id === budget.model_id)?.id
    return `model:${key ?? budget.model_id}`
  }
  return `upstream:${budget.upstream_model ?? ''}`
}

export function formatCadence(cadence: string) {
  return cadence.charAt(0).toUpperCase() + cadence.slice(1)
}
