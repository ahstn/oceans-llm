import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { ApiKeyOwnerUserView, McpGrantView, McpServerView, McpToolsetView } from '@/types/api'

const invalidateMock = vi.fn()
const getMcpServerToolsMock = vi.fn()
const saveMcpGrantMock = vi.fn()
const removeMcpGrantMock = vi.fn()
const getMcpEffectiveAccessMock = vi.fn()

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => ({ useLoaderData: vi.fn(), useSearch: vi.fn() }),
  useRouter: () => ({ navigate: vi.fn(), invalidate: invalidateMock }),
}))

vi.mock('sonner', () => ({ toast: { error: vi.fn(), success: vi.fn() } }))

vi.mock('@/server/admin-data.functions', () => ({
  getMcpEffectiveAccess: (...args: unknown[]) => getMcpEffectiveAccessMock(...args),
  getMcpServerTools: (...args: unknown[]) => getMcpServerToolsMock(...args),
  removeMcpGrant: (...args: unknown[]) => removeMcpGrantMock(...args),
  saveMcpGrant: (...args: unknown[]) => saveMcpGrantMock(...args),
}))

const server: McpServerView = {
  id: 'server_1',
  server_key: 'github',
  display_name: 'GitHub',
  description: null,
  transport: 'streamable_http',
  server_url: 'https://api.githubcopilot.com/mcp/',
  auth_mode: 'none',
  auth_config: {},
  timeout_ms: 30000,
  status: 'active',
  last_discovery_status: 'success',
  last_discovery_at: null,
  last_successful_discovery_at: null,
  last_error_summary: null,
  last_tool_count: 0,
  created_at: '2026-05-27T08:00:00Z',
  updated_at: '2026-05-27T09:00:00Z',
  disabled_at: null,
}

const toolset: McpToolsetView = {
  id: 'toolset_1',
  toolset_key: 'github-readonly',
  display_name: 'GitHub read-only',
  description: null,
  status: 'active',
  created_at: '2026-05-27T08:00:00Z',
  updated_at: '2026-05-27T09:00:00Z',
  disabled_at: null,
}

const grant: McpGrantView = {
  id: 'grant_1',
  subject_kind: 'user',
  subject_id: 'user_1',
  target_kind: 'toolset',
  target_id: 'toolset_1',
  is_active: true,
  revoked_at: null,
  created_at: '2026-05-27T08:00:00Z',
  updated_at: '2026-05-27T09:00:00Z',
}

const user: ApiKeyOwnerUserView = {
  id: 'user_1',
  name: 'Jane Admin',
  email: 'jane@example.com',
}

async function renderAccessTab() {
  const { AccessTab } = await import('@/routes/mcp/-access-tab')
  render(
    <AccessTab
      grants={[grant]}
      servers={[server]}
      toolsets={[toolset]}
      subjects={{ apiKeys: [], users: [user], serviceAccounts: [], teams: [] }}
    />,
  )
}

describe('AccessTab', () => {
  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', ResizeObserverMock)
    vi.stubGlobal('matchMedia', vi.fn().mockImplementation((query: string) => ({
      matches: true,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })))

    invalidateMock.mockReset()
    getMcpServerToolsMock.mockReset()
    saveMcpGrantMock.mockReset()
    removeMcpGrantMock.mockReset()
    getMcpEffectiveAccessMock.mockReset()
    getMcpServerToolsMock.mockResolvedValue({ data: { items: [] } })
  })

  it('resolves grant subject and target UUIDs to human labels', async () => {
    await renderAccessTab()

    expect(screen.getByText('Jane Admin')).toBeInTheDocument()
    expect(screen.getByText('GitHub read-only')).toBeInTheDocument()
    expect(screen.getByText('New grant')).toBeInTheDocument()
    expect(screen.getByText('Effective access')).toBeInTheDocument()
  })

  it('revokes a grant through the server function', async () => {
    await renderAccessTab()
    fireEvent.click(screen.getByRole('button', { name: 'Revoke' }))

    await waitFor(() => {
      expect(removeMcpGrantMock).toHaveBeenCalledWith({
        data: {
          subject_kind: 'user',
          subject_id: 'user_1',
          target_kind: 'toolset',
          target_id: 'toolset_1',
        },
      })
    })
  })
})
