import type { McpToolsetView, McpToolView } from '@/types/api'
import type { ToolsetWorkbenchProps } from '@/components/mcp/toolset-workbench'
import { sampleServers } from '../mcp-designs/model'

export type ToolsetFilter = 'all' | 'active' | 'disabled'
export type CatalogState = 'ready' | 'loading' | 'error'
export type ToolsetCandidate = 'directory' | 'workbench' | 'guided'

export interface ToolsetCandidateProps {
  sets: McpToolsetView[]
  allSets: McpToolsetView[]
  query: string
  filter: ToolsetFilter
  onQueryChange: (query: string) => void
  onFilterChange: (filter: ToolsetFilter) => void
  selected: McpToolsetView | null
  onSelect: (set: McpToolsetView | null) => void
  draftIds: string[]
  draftSaved?: boolean
  onToggleTool: (id: string, checked: boolean) => void
  onClearDraft: () => void
  catalogState: CatalogState
  onRetryCatalog: () => void
  onCreate: () => void
  onEditMetadata: () => void
  onDisable: () => void
  onReview: () => void
  onAccess: () => void
  memberships: ToolsetWorkbenchProps['memberships']
  onEditSet: (id: string) => void
  onSaveSet: (id: string) => void
  onDisableSet: (id: string) => void
  workspaceRevision: number
}

export type ToolsetDetails = Pick<McpToolsetView, 'display_name' | 'toolset_key' | 'description'>

const timestamp = '2026-09-05T09:42:00Z'

// Synthetic metadata and membership are kept separately, as in the gateway API.
export const sampleToolsets: McpToolsetView[] = [
  [
    'engineering',
    'engineering-essentials',
    'Engineering essentials',
    'Repository context and project knowledge for engineering agents.',
  ],
  [
    'research',
    'research-desk',
    'Research desk',
    'Source discovery and reference material for grounded research.',
  ],
  [
    'support',
    'support-knowledge',
    'Support knowledge',
    'Workspace guides and service documentation for support teams.',
  ],
  [
    'release',
    'release-operations',
    'Release operations',
    'Issues, release notes, and deployment documentation.',
  ],
  [
    'legacy',
    'legacy-docs',
    'Legacy documentation',
    'Retired documentation bundle. Kept for reference.',
  ],
].map(([id, toolset_key, display_name, description], index) => ({
  id,
  toolset_key,
  display_name,
  description,
  status: id === 'legacy' ? 'disabled' : 'active',
  created_at: '2026-09-01T10:00:00Z',
  updated_at: `2026-09-0${5 - index}T09:42:00Z`,
  disabled_at: id === 'legacy' ? timestamp : null,
}))

function tool(
  server_id: string,
  upstream_name: string,
  display_name: string,
  description: string,
  is_active = true,
): McpToolView {
  return {
    id: `${server_id}-${upstream_name}`,
    server_id,
    upstream_name,
    display_name,
    description,
    is_active,
    input_schema: {
      type: 'object',
      properties: {
        query: { type: 'string', description: 'The search query or resource identifier.' },
        limit: { type: 'integer', minimum: 1, maximum: 100 },
      },
      required: ['query'],
    },
    first_discovered_at: timestamp,
    last_discovered_at: timestamp,
    schema_hash: 'sample-only',
    schema_version: 1,
    deactivated_at: is_active ? null : timestamp,
  }
}

export const sampleTools: McpToolView[] = [
  tool(
    'github',
    'search_repositories',
    'Search repositories',
    'Find repositories by name, topic, or owner.',
  ),
  tool(
    'github',
    'get_pull_request',
    'Get pull request',
    'Read a pull request, its changes, and review context.',
  ),
  tool(
    'github',
    'list_issues',
    'List issues',
    'Browse issues and filter by label, owner, or state.',
  ),
  tool(
    'github',
    'create_issue',
    'Create issue',
    'Open an issue in a repository with a title and body.',
  ),
  tool(
    'github',
    'legacy_code_search',
    'Legacy code search',
    'An older search tool that is no longer available.',
    false,
  ),
  tool(
    'notion',
    'search_pages',
    'Search pages',
    'Find pages and databases in the connected workspace.',
  ),
  tool(
    'notion',
    'fetch_page',
    'Fetch page',
    'Read the content and properties of a workspace page.',
  ),
  tool('notion', 'update_page', 'Update page', 'Change the content or properties of a page.'),
  tool('exa', 'web_search', 'Web search', 'Find current sources across the web.'),
  tool('exa', 'get_contents', 'Get contents', 'Retrieve readable source material from a URL.'),
  tool(
    'cloudflare',
    'search_docs',
    'Search documentation',
    'Find guides and API references for Cloudflare services.',
  ),
]

export const sampleMemberships: Record<string, string[]> = {
  engineering: ['github-search_repositories', 'github-get_pull_request', 'notion-search_pages'],
  research: ['exa-web_search', 'exa-get_contents'],
  support: ['notion-fetch_page', 'cloudflare-search_docs'],
  release: ['github-list_issues', 'github-create_issue', 'notion-update_page'],
  legacy: ['github-legacy_code_search', 'notion-fetch_page'],
}

export interface PreviewMembership {
  savedIds: string[]
  draftIds: string[]
}

export function initialMemberships(): Record<string, PreviewMembership> {
  return Object.fromEntries(
    Object.entries(sampleMemberships).map(([id, toolIds]) => [
      id,
      { savedIds: [...toolIds], draftIds: [...toolIds] },
    ]),
  )
}

export function membershipIsDirty(membership: PreviewMembership) {
  return (
    membership.savedIds.length !== membership.draftIds.length ||
    membership.savedIds.some((id) => !membership.draftIds.includes(id))
  )
}

export const toolGroups = sampleServers
  .map((server) => ({ server, tools: sampleTools.filter((item) => item.server_id === server.id) }))
  .filter((group) => group.tools.length > 0)

export function filterToolsets(sets: McpToolsetView[], query: string, filter: ToolsetFilter) {
  const term = query.trim().toLowerCase()
  return sets.filter(
    (set) =>
      (filter === 'all' || set.status === filter) &&
      `${set.display_name} ${set.toolset_key} ${set.description ?? ''}`
        .toLowerCase()
        .includes(term),
  )
}

export function selectedTools(ids: string[]) {
  return sampleTools.filter((item) => ids.includes(item.id))
}

export function initialCandidate(): ToolsetCandidate {
  const candidate = new URLSearchParams(window.location.search).get('candidate')
  return candidate === 'directory' || candidate === 'guided' ? candidate : 'workbench'
}
