import type { ApiKeyView } from '@/types/api'

// Capability scopes and MCP server lists on API keys, shared by the API keys page and its
// dialogs. See docs/adr/2026-10-17-scoped-api-keys.md.

/** A key's scopes and MCP server list. `null` is full access, or every server. */
export type ApiKeyScopesValue = {
  scopes: string[] | null
  mcp_server_keys: string[] | null
}

export const fullAccessScopes: ApiKeyScopesValue = { scopes: null, mcp_server_keys: null }

const MCP_SCOPES = new Set(['mcp:aggregate', 'mcp:proxy'])

export const SCOPE_DETAILS: Record<string, { label: string; description: string }> = {
  inference: {
    label: 'Inference',
    description: 'Model calls, model listing, batches, and guardrail checks.',
  },
  'mcp:aggregate': {
    label: 'MCP aggregate',
    description: 'The /mcp endpoint: search_tools, describe_tool, and call_tool.',
  },
  'mcp:proxy': {
    label: 'MCP proxy',
    description: 'Per-server /mcp/{server_key} routes.',
  },
}

export function scopeLabel(scope: string) {
  return SCOPE_DETAILS[scope]?.label ?? scope
}

export function scopesOf(item: Pick<ApiKeyView, 'scopes' | 'mcp_server_keys'>): ApiKeyScopesValue {
  return { scopes: item.scopes ?? null, mcp_server_keys: item.mcp_server_keys ?? null }
}

/** One line for listings, such as "MCP proxy · notes, wiki". */
export function formatScopeSummary(item: Pick<ApiKeyView, 'scopes' | 'mcp_server_keys'>) {
  const { scopes, mcp_server_keys } = scopesOf(item)
  const capabilities = scopes === null ? 'Full access' : scopes.map(scopeLabel).join(', ')
  return mcp_server_keys === null ? capabilities : `${capabilities} · ${mcp_server_keys.join(', ')}`
}

/** Whether a server list may be set: it needs full access or an MCP scope. */
export function allowsMcp(value: ApiKeyScopesValue) {
  return value.scopes === null || value.scopes.some((scope) => MCP_SCOPES.has(scope))
}

/** Whether a limited key is missing its scopes or servers. */
export function isScopesValueIncomplete(value: ApiKeyScopesValue) {
  return (
    (value.scopes !== null && value.scopes.length === 0) ||
    (value.mcp_server_keys !== null && value.mcp_server_keys.length === 0)
  )
}

export function sameScopesValue(left: ApiKeyScopesValue, right: ApiKeyScopesValue) {
  return (
    sameList(left.scopes, right.scopes) && sameList(left.mcp_server_keys, right.mcp_server_keys)
  )
}

function sameList(left: string[] | null, right: string[] | null) {
  if (left === null || right === null) {
    return left === right
  }
  const rightValues = new Set(right)
  return left.length === rightValues.size && left.every((value) => rightValues.has(value))
}

/** Adds or removes `value`, keeping each entry once. */
export function toggleValue(values: string[], value: string, on: boolean) {
  const rest = values.filter((existing) => existing !== value)
  return on ? [...rest, value] : rest
}
