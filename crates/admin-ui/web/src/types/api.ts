export interface ApiEnvelope<T> {
  data: T
  meta?: {
    generatedAt?: string
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

export interface RequestLogView {
  id: string
  model: string
  provider: string
  statusCode: number
  latencyMs: number
  tokens: number
  timestamp: string
}

export interface TeamView {
  id: string
  name: string
  users: number
  status: 'active' | 'inactive'
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
