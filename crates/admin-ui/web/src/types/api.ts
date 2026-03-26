export interface ApiEnvelope<T> {
  data: T
  meta?: {
    generated_at?: string
  }
}

export interface Paginated<T> {
  items: T[]
  page: number
  pageSize: number
  total: number
}

export interface ApiKeyView {
  id: string
  name: string
  prefix: string
  createdAt: string
  status: 'active' | 'revoked'
}

export interface ModelView {
  id: string
  provider: string
  upstreamModel: string
  tags: string[]
  status: 'healthy' | 'degraded'
}

export interface UsageCostPoint {
  day: string
  amountUsd: number
}

export type SpendOwnerKind = 'all' | 'user' | 'team'

export interface SpendTotalsView {
  priced_cost_usd_10000: number
  priced_request_count: number
  unpriced_request_count: number
  usage_missing_request_count: number
}

export interface SpendDailyPointView {
  day_start: string
  priced_cost_usd_10000: number
  priced_request_count: number
  unpriced_request_count: number
  usage_missing_request_count: number
}

export interface SpendOwnerBreakdownView {
  owner_kind: 'user' | 'team'
  owner_id: string
  owner_name: string
  priced_cost_usd_10000: number
  priced_request_count: number
  unpriced_request_count: number
  usage_missing_request_count: number
}

export interface SpendModelBreakdownView {
  model_key: string
  priced_cost_usd_10000: number
  priced_request_count: number
  unpriced_request_count: number
  usage_missing_request_count: number
}

export interface SpendReportView {
  window_days: number
  owner_kind: SpendOwnerKind
  window_start: string
  window_end: string
  totals: SpendTotalsView
  daily: SpendDailyPointView[]
  owners: SpendOwnerBreakdownView[]
  models: SpendModelBreakdownView[]
}

export interface BudgetSettingsView {
  cadence: 'daily' | 'weekly' | 'monthly'
  amount_usd: string
  amount_usd_10000: number
  hard_limit: boolean
  timezone: string
}

export interface SpendBudgetUserView {
  user_id: string
  name: string
  email: string
  team_id: string | null
  team_name: string | null
  budget: BudgetSettingsView | null
  current_window_spend_usd_10000: number
  alert_email_ready: boolean
  alert_recipient_summary: string
}

export interface SpendBudgetTeamView {
  team_id: string
  team_name: string
  team_key: string
  budget: BudgetSettingsView | null
  current_window_spend_usd_10000: number
  alert_email_ready: boolean
  alert_recipient_summary: string
}

export interface SpendBudgetsView {
  users: SpendBudgetUserView[]
  teams: SpendBudgetTeamView[]
}

export interface UpsertBudgetInput {
  cadence: 'daily' | 'weekly' | 'monthly'
  amount_usd: string
  hard_limit: boolean
  timezone?: string | null
}

export interface BudgetAlertHistoryItemView {
  budget_alert_id: string
  owner_kind: 'user' | 'team'
  owner_id: string
  owner_name: string
  channel: 'email'
  delivery_status: 'pending' | 'sent' | 'failed'
  recipient_summary: string
  threshold_bps: number
  cadence: 'daily' | 'weekly' | 'monthly'
  window_start: string
  window_end: string
  spend_before_usd_10000: number
  spend_after_usd_10000: number
  remaining_budget_usd_10000: number
  created_at: string
  last_attempted_at: string | null
  sent_at: string | null
  failure_reason: string | null
}

export interface BudgetAlertHistoryView {
  items: BudgetAlertHistoryItemView[]
  page: number
  page_size: number
  total: number
}

export interface UpsertBudgetResultView {
  owner_kind: 'user' | 'team'
  owner_id: string
  budget: BudgetSettingsView
  current_window_spend_usd_10000: number
}

export interface DeactivateBudgetResultView {
  owner_kind: 'user' | 'team'
  owner_id: string
  deactivated: boolean
}

export interface RequestLogView {
  requestLogId: string
  requestId: string
  apiKeyId: string
  userId: string | null
  teamId: string | null
  modelKey: string
  resolvedModelKey: string
  providerKey: string
  statusCode: number | null
  latencyMs: number | null
  promptTokens: number | null
  completionTokens: number | null
  totalTokens: number | null
  errorCode: string | null
  hasPayload: boolean
  requestPayloadTruncated: boolean
  responsePayloadTruncated: boolean
  requestTags: RequestLogTagsView
  metadata: Record<string, unknown>
  occurredAt: string
}

export interface RequestTagView {
  key: string
  value: string
}

export interface RequestLogTagsView {
  service: string | null
  component: string | null
  env: string | null
  bespoke: RequestTagView[]
}

export interface RequestLogPayloadView {
  requestJson: unknown
  responseJson: unknown
}

export interface RequestLogDetailView {
  log: RequestLogView
  payload: RequestLogPayloadView | null
}

export interface RequestLogFiltersInput {
  requestId?: string
  modelKey?: string
  providerKey?: string
  service?: string
  component?: string
  env?: string
  tagKey?: string
  tagValue?: string
}

export interface TeamAdminView {
  id: string
  name: string
  email: string
  status: 'active' | 'invited' | 'disabled'
}

export interface TeamManagementView {
  id: string
  name: string
  key: string
  status: 'active' | 'inactive'
  member_count: number
  admins: TeamAdminView[]
}

export interface TeamAssignableUserView {
  id: string
  name: string
  email: string
  status: 'active' | 'invited' | 'disabled'
  team_id: string | null
  team_name: string | null
  team_role: 'owner' | 'admin' | 'member' | null
}

export interface IdentityTeamsPayload {
  teams: TeamManagementView[]
  users: TeamAssignableUserView[]
  oidc_providers: OidcProviderView[]
}

export interface CreateTeamInput {
  name: string
  admin_user_ids: string[]
}

export interface UpdateTeamInput {
  name: string
  admin_user_ids: string[]
}

export interface AddTeamMembersInput {
  user_ids: string[]
}

export interface RemoveTeamMemberResult {
  status: 'ok'
}

export interface TransferTeamMemberInput {
  destination_team_id: string
  destination_role: 'member' | 'admin'
}

export interface TransferTeamMemberResult {
  status: 'ok'
}

export interface AdminTeamOption {
  id: string
  name: string
}

export interface OidcProviderView {
  id: string
  key: string
  label: string
}

export type UserOnboardingAction =
  | {
      kind: 'password_invite'
      invite_url: string | null
      expires_at: string | null
      can_resend: boolean
    }
  | {
      kind: 'oidc_sign_in'
      sign_in_url: string
      provider_key: string
      provider_label: string
    }

export interface UserView {
  id: string
  name: string
  email: string
  auth_mode: 'password' | 'oidc'
  global_role: 'platform_admin' | 'user'
  team_id: string | null
  team_name: string | null
  team_role: 'owner' | 'admin' | 'member' | null
  status: 'active' | 'invited' | 'disabled'
  onboarding?: UserOnboardingAction | null
}

export interface IdentityUsersPayload {
  users: UserView[]
  teams: AdminTeamOption[]
  oidc_providers: OidcProviderView[]
}

export interface CreateUserInput {
  name: string
  email: string
  auth_mode: 'password' | 'oidc'
  global_role: 'platform_admin' | 'user'
  team_id?: string | null
  team_role?: 'owner' | 'admin' | 'member' | null
  oidc_provider_key?: string | null
}

export interface UpdateUserInput {
  global_role?: 'platform_admin' | 'user'
  team_id?: string | null
  team_role?: 'admin' | 'member' | null
  auth_mode?: 'password' | 'oidc'
  oidc_provider_key?: string | null
}

export interface IdentityActionResult {
  status: 'ok'
}

export type CreateUserResult =
  | {
      kind: 'password_invite'
      user: UserView
      invite_url: string
      expires_at: string
    }
  | {
      kind: 'oidc_sign_in'
      user: UserView
      sign_in_url: string
      provider_label: string
    }

export interface PasswordInviteResult {
  user_id: string
  invite_url: string
  expires_at: string
}

export interface InvitationStateView {
  state: 'valid' | 'expired' | 'consumed' | 'revoked' | 'invalid'
  email: string | null
  name: string | null
  expires_at: string | null
}

export interface AuthSessionUserView {
  id: string
  name: string
  email: string
  global_role: 'platform_admin' | 'user'
}

export interface AuthSessionView {
  user: AuthSessionUserView
  must_change_password: boolean
}

export interface PasswordLoginInput {
  email: string
  password: string
}

export interface ChangePasswordInput {
  current_password: string
  new_password: string
}
