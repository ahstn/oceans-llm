import { toast } from 'sonner'

import type {
  CreateMcpServerInput,
  McpServerView,
  RecommendedMcpServerView,
  UpdateMcpServerInput,
  UpsertMcpCredentialBindingInput,
} from '@/types/api'

export type ServerFormState = {
  server_key: string
  display_name: string
  description: string
  server_url: string
  auth_mode: string
  auth_config: string
  timeout_ms: string
  visibility: ServerVisibility
  /** Blank on create defaults to the caller's email; blank on edit clears it. */
  owner_contact: string
  /** A platform admin's choice of owning team on create; blank is the platform. */
  owner_team_id: string
  approve_tool_changes: boolean
}

export type ServerVisibility = 'team' | 'organisation'

export type CredentialBindingFormState = {
  owner_scope_kind: 'user' | 'team' | 'service_account'
  owner_user_id: string
  owner_team_id: string
  owner_service_account_id: string
  material_kind: 'static_header' | 'bearer_token' | 'oauth_tokens'
  header_name: string
  storage_mode: 'secret' | 'secret_ref'
  secret: string
  secret_ref: string
  expires_at: string
}

/** Team admins register for their team, so their servers start team-only. */
export function emptyServerForm(visibility: ServerVisibility = 'organisation'): ServerFormState {
  return {
    server_key: '',
    display_name: '',
    description: '',
    server_url: '',
    auth_mode: 'none',
    auth_config: '{}',
    timeout_ms: '30000',
    visibility,
    owner_contact: '',
    owner_team_id: '',
    approve_tool_changes: false,
  }
}

/** A team admin sets only their team's shared credential or a service account's. */
export function emptyCredentialBindingForm(teamId?: string | null): CredentialBindingFormState {
  return {
    owner_scope_kind: teamId ? 'team' : 'user',
    owner_user_id: '',
    owner_team_id: teamId ?? '',
    owner_service_account_id: '',
    material_kind: 'bearer_token',
    header_name: '',
    storage_mode: 'secret',
    secret: '',
    secret_ref: '',
    expires_at: '',
  }
}

export function formFromRecommended(
  server: RecommendedMcpServerView,
  visibility: ServerVisibility = 'organisation',
): ServerFormState {
  return {
    visibility,
    owner_contact: '',
    owner_team_id: '',
    server_key: server.catalog_key,
    display_name: server.display_name,
    description: server.description ?? '',
    server_url: server.server_url,
    auth_mode: server.auth_mode,
    auth_config: JSON.stringify(server.auth_config ?? {}, null, 2),
    timeout_ms: '30000',
    approve_tool_changes: false,
  }
}

export function formFromServer(server: McpServerView): ServerFormState {
  return {
    server_key: server.server_key,
    display_name: server.display_name,
    description: server.description ?? '',
    server_url: server.server_url,
    auth_mode: server.auth_mode,
    auth_config: JSON.stringify(server.auth_config ?? {}, null, 2),
    timeout_ms: String(server.timeout_ms),
    visibility: server.visibility === 'team' ? 'team' : 'organisation',
    owner_contact: server.owner_contact ?? '',
    owner_team_id: server.owner_team_id ?? '',
    approve_tool_changes: server.approve_tool_changes,
  }
}

export function formToCreateInput(form: ServerFormState): CreateMcpServerInput | null {
  const authConfig = parseAuthConfig(form.auth_config)
  if (!authConfig) {
    return null
  }
  return {
    server_key: form.server_key.trim(),
    display_name: form.display_name.trim(),
    description: optionalString(form.description),
    server_url: form.server_url.trim(),
    transport: 'streamable_http',
    auth_mode: form.auth_mode,
    auth_config: authConfig,
    timeout_ms: optionalNumber(form.timeout_ms),
    visibility: form.visibility,
    owner_contact: optionalString(form.owner_contact),
    // A team admin leaves it blank; the gateway registers for their team.
    owner_team_id: optionalString(form.owner_team_id),
    approve_tool_changes: form.approve_tool_changes,
  }
}

export function formToUpdateInput(form: ServerFormState): UpdateMcpServerInput | null {
  const authConfig = parseAuthConfig(form.auth_config)
  if (!authConfig) {
    return null
  }
  return {
    display_name: form.display_name.trim(),
    description: optionalString(form.description),
    server_url: form.server_url.trim(),
    auth_mode: form.auth_mode,
    auth_config: authConfig,
    timeout_ms: optionalNumber(form.timeout_ms),
    visibility: form.visibility,
    // An empty string clears the contact.
    owner_contact: form.owner_contact.trim(),
    approve_tool_changes: form.approve_tool_changes,
  }
}

export function formToCredentialBindingInput(
  serverId: string,
  form: CredentialBindingFormState,
): UpsertMcpCredentialBindingInput | null {
  const expiresAt = optionalDateTime(form.expires_at)
  if (expiresAt === undefined) {
    return null
  }
  return {
    server_id: serverId,
    owner_scope_kind: form.owner_scope_kind,
    owner_user_id: form.owner_scope_kind === 'user' ? requiredString(form.owner_user_id) : null,
    owner_team_id:
      form.owner_scope_kind === 'team' || form.owner_scope_kind === 'service_account'
        ? requiredString(form.owner_team_id)
        : null,
    owner_service_account_id:
      form.owner_scope_kind === 'service_account'
        ? requiredString(form.owner_service_account_id)
        : null,
    material_kind: form.material_kind,
    header_name: form.material_kind === 'static_header' ? requiredString(form.header_name) : null,
    secret: form.storage_mode === 'secret' ? requiredString(form.secret) : null,
    secret_ref: form.storage_mode === 'secret_ref' ? requiredString(form.secret_ref) : null,
    expires_at: expiresAt,
    metadata: {},
  }
}

function parseAuthConfig(value: string): Record<string, unknown> | null {
  try {
    const parsed = JSON.parse(value || '{}') as unknown
    if (!parsed || Array.isArray(parsed) || typeof parsed !== 'object') {
      toast.error('Auth config must be a JSON object')
      return null
    }
    return parsed as Record<string, unknown>
  } catch {
    toast.error('Auth config is not valid JSON')
    return null
  }
}

function optionalString(value: string) {
  const trimmed = value.trim()
  return trimmed.length > 0 ? trimmed : null
}

function optionalNumber(value: string) {
  const trimmed = value.trim()
  return trimmed.length > 0 ? Number(trimmed) : null
}

function requiredString(value: string) {
  const trimmed = value.trim()
  return trimmed.length > 0 ? trimmed : null
}

function optionalDateTime(value: string): string | null | undefined {
  const trimmed = value.trim()
  if (trimmed.length === 0) {
    return null
  }
  const date = new Date(trimmed)
  if (Number.isNaN(date.getTime())) {
    toast.error('Credential expiry is not a valid date')
    return undefined
  }
  return date.toISOString()
}
