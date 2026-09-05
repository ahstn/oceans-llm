import type { McpServerView } from '@/types/api'

export type ServerFilter = 'all' | 'attention' | 'active' | 'disabled'
export type DetailSection = 'overview' | 'configuration' | 'tools' | 'credentials'

export interface CandidateProps {
  servers: McpServerView[]
  allServers: McpServerView[]
  query: string
  filter: ServerFilter
  onQueryChange: (query: string) => void
  onFilterChange: (filter: ServerFilter) => void
  onManage: (server: McpServerView, section?: DetailSection) => void
  onAdd: () => void
  onCatalog: () => void
  onRefresh: (server: McpServerView) => void
  refreshingId: string | null
}

export function needsAttention(server: McpServerView) {
  return (
    server.status === 'active' &&
    ['failed', 'auth_required'].includes(server.last_discovery_status ?? '')
  )
}

export function filterServers(servers: McpServerView[], query: string, filter: ServerFilter) {
  const search = query.trim().toLowerCase()
  return servers.filter((server) => {
    const matches =
      `${server.display_name} ${server.server_key} ${server.server_url} ${server.description ?? ''}`
        .toLowerCase()
        .includes(search)
    return (
      matches &&
      (filter === 'all' ||
        (filter === 'attention' ? needsAttention(server) : server.status === filter))
    )
  })
}

export function authLabel(mode: string) {
  return (
    (
      {
        none: 'No authentication',
        gateway_bearer_token: 'Gateway bearer token',
        oauth_obo: 'OAuth on-behalf-of',
        gateway_static_header: 'Gateway static header',
        user_passthrough: 'User passthrough',
      } as Record<string, string>
    )[mode] ?? mode
  )
}

export function endpointHost(server: McpServerView) {
  try {
    return new URL(server.server_url).hostname
  } catch {
    return server.server_url
  }
}

export function discoveryLabel(server: McpServerView) {
  if (server.last_discovery_status === 'failed') return 'Discovery failed'
  if (server.last_discovery_status === 'auth_required') return 'Authentication required'
  if (server.last_discovery_status === 'disabled') return 'Discovery disabled'
  if (server.last_discovery_status === 'success') return 'Discovered'
  return 'Not discovered'
}

export function discoveredAt(server: McpServerView) {
  if (!server.last_discovery_at) return 'No discovery yet'
  return formatTimestamp(server.last_discovery_at)
}

export function formatTimestamp(value?: string | null) {
  if (!value) return 'Never'
  return (
    new Intl.DateTimeFormat('en-GB', {
      day: 'numeric',
      month: 'short',
      hour: '2-digit',
      minute: '2-digit',
      timeZone: 'UTC',
    }).format(new Date(value)) + ' UTC'
  )
}

export function createPreviewServer(
  template: Pick<McpServerView, 'display_name' | 'server_key' | 'server_url'> &
    Partial<McpServerView>,
): McpServerView {
  const now = new Date().toISOString()
  return {
    ...template,
    id: `preview-${crypto.randomUUID()}`,
    status: 'active',
    auth_mode: template.auth_mode ?? 'none',
    auth_config: {},
    transport: 'streamable_http',
    timeout_ms: 30000,
    created_at: now,
    updated_at: now,
    disabled_at: null,
    last_discovery_status: null,
    last_discovery_at: null,
    last_successful_discovery_at: null,
    last_error_summary: null,
    last_tool_count: null,
  }
}

const base: McpServerView = {
  id: '',
  server_key: '',
  display_name: '',
  server_url: '',
  auth_mode: 'gateway_bearer_token',
  auth_config: {},
  status: 'active',
  transport: 'streamable_http',
  timeout_ms: 30000,
  created_at: '2026-09-01T10:00:00Z',
  updated_at: '2026-09-05T09:42:00Z',
  last_discovery_status: 'success',
  last_discovery_at: '2026-09-05T09:42:00Z',
  last_successful_discovery_at: '2026-09-05T09:42:00Z',
  last_tool_count: 12,
}

// Fixed, synthetic fixtures use the gateway contract. No provider calls are made in the preview.
export const sampleServers: McpServerView[] = [
  {
    ...base,
    id: 'github',
    server_key: 'github',
    display_name: 'GitHub',
    server_url: 'https://api.githubcopilot.com/mcp/',
    description: 'Repositories, pull requests, and issues for your engineering tools.',
    last_tool_count: 40,
  },
  {
    ...base,
    id: 'notion',
    server_key: 'notion',
    display_name: 'Notion',
    server_url: 'https://mcp.notion.com/mcp',
    description: 'Search workspace knowledge and keep project documents in sync.',
    auth_mode: 'oauth_obo',
    last_tool_count: 18,
  },
  {
    ...base,
    id: 'exa',
    server_key: 'exa',
    display_name: 'Exa',
    server_url: 'https://mcp.exa.ai/mcp',
    description: 'Search the web and retrieve source material for research.',
    last_tool_count: 8,
  },
  {
    ...base,
    id: 'figma',
    server_key: 'figma',
    display_name: 'Figma',
    server_url: 'https://mcp.figma.com/mcp',
    description: 'Bring design context, components, and layout details into your tools.',
    auth_mode: 'oauth_obo',
    last_discovery_status: 'auth_required',
    last_error_summary:
      'Authentication failed (401). The credential may have expired. Review credentials, then retry discovery.',
    last_successful_discovery_at: '2026-09-04T16:20:00Z',
    last_tool_count: 12,
  },
  {
    ...base,
    id: 'cloudflare',
    server_key: 'cloudflare-docs',
    display_name: 'Cloudflare',
    server_url: 'https://docs.mcp.cloudflare.com/mcp',
    description: 'Find documentation for Workers, storage, and network services.',
    auth_mode: 'none',
    last_tool_count: 3,
  },
  {
    ...base,
    id: 'internal',
    server_key: 'internal-docs',
    display_name: 'Internal documentation',
    server_url: 'https://mcp.internal.example.com/documentation/mcp',
    description: 'Private service documentation and operational runbooks.',
    status: 'disabled',
    disabled_at: '2026-09-04T12:00:00Z',
    last_discovery_status: null,
    last_discovery_at: null,
    last_successful_discovery_at: null,
    last_tool_count: null,
  },
]

// Extra sample templates make catalog registration reviewable without live provider calls.
export const sampleCatalog: McpServerView[] = [
  {
    ...base,
    id: 'catalog-hugging-face',
    server_key: 'hugging-face',
    display_name: 'Hugging Face',
    server_url: 'https://hugging-face.example.com/mcp',
    description: 'Sample model, dataset, and research discovery service.',
    last_discovery_status: null,
    last_discovery_at: null,
    last_successful_discovery_at: null,
    last_tool_count: null,
  },
  {
    ...base,
    id: 'catalog-snowflake',
    server_key: 'snowflake',
    display_name: 'Snowflake',
    server_url: 'https://snowflake.example.com/mcp',
    description: 'Sample warehouse queries and data catalog tools.',
    last_discovery_status: null,
    last_discovery_at: null,
    last_successful_discovery_at: null,
    last_tool_count: null,
  },
  ...sampleServers.filter((server) => server.id !== 'internal'),
]

export const sampleTools = [
  {
    name: 'search',
    description: 'Find matching records in this service.',
    schema: {
      type: 'object',
      properties: {
        query: { type: 'string', description: 'Search terms' },
        limit: { type: 'integer', minimum: 1, maximum: 100 },
      },
      required: ['query'],
    },
  },
  {
    name: 'get_item',
    description: 'Read a single record by its identifier.',
    schema: {
      type: 'object',
      properties: { id: { type: 'string', description: 'The record identifier' } },
      required: ['id'],
    },
  },
  {
    name: 'list_items',
    description: 'List records with an optional page cursor.',
    schema: {
      type: 'object',
      properties: { cursor: { type: 'string' }, limit: { type: 'integer', default: 20 } },
    },
  },
]
