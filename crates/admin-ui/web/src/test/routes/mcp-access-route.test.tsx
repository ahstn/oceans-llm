import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type {
  ApiKeyOwnerUserView,
  McpGrantView,
  McpServerView,
  McpTagRuleView,
  McpToolsetView,
} from '@/types/api'

const invalidateMock = vi.fn()
const getMcpServerToolsMock = vi.fn()
const saveMcpGrantMock = vi.fn()
const removeMcpGrantMock = vi.fn()
const getMcpEffectiveAccessMock = vi.fn()
const saveMcpTagRuleMock = vi.fn()
const removeMcpTagRuleMock = vi.fn()
const getMcpTagRulePreviewMock = vi.fn()

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

vi.mock('@/server/mcp-policy-data.functions', () => ({
  getMcpTagRulePreview: (...args: unknown[]) => getMcpTagRulePreviewMock(...args),
  removeMcpTagRule: (...args: unknown[]) => removeMcpTagRuleMock(...args),
  saveMcpTagRule: (...args: unknown[]) => saveMcpTagRuleMock(...args),
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

const tagRule: McpTagRuleView = {
  id: 'rule_1',
  subject_kind: 'user',
  subject_id: 'user_1',
  server_id: null,
  tag: 'destructive',
  effect: 'exclude',
  is_active: true,
  created_at: '2026-05-27T08:00:00Z',
  updated_at: '2026-05-27T08:00:00Z',
  revoked_at: null,
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
      tagRules={[tagRule]}
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
    vi.stubGlobal(
      'matchMedia',
      vi.fn().mockImplementation((query: string) => ({
        matches: true,
        media: query,
        onchange: null,
        addListener: vi.fn(),
        removeListener: vi.fn(),
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    )

    invalidateMock.mockReset()
    getMcpServerToolsMock.mockReset()
    saveMcpGrantMock.mockReset()
    removeMcpGrantMock.mockReset()
    getMcpEffectiveAccessMock.mockReset()
    saveMcpTagRuleMock.mockReset()
    removeMcpTagRuleMock.mockReset()
    getMcpTagRulePreviewMock.mockReset()
    getMcpServerToolsMock.mockResolvedValue({ data: { items: [] } })
  })

  it('resolves grant subject and target UUIDs to human labels', async () => {
    await renderAccessTab()

    const grants = screen.getByTestId('mcp-grant-list')
    expect(within(grants).getByText('Jane Admin')).toBeInTheDocument()
    expect(within(grants).getByText('GitHub read-only')).toBeInTheDocument()
    expect(screen.getByText('New grant')).toBeInTheDocument()
    expect(screen.getByText('Effective access')).toBeInTheDocument()
  })

  it('revokes a grant through the server function', async () => {
    await renderAccessTab()
    fireEvent.click(
      within(screen.getByTestId('mcp-grant-list')).getByRole('button', { name: 'Revoke' }),
    )

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

  it('lists active tag rules and revokes one', async () => {
    await renderAccessTab()
    const rules = screen.getByTestId('mcp-tag-rule-list')
    expect(within(rules).getByText('Jane Admin')).toBeInTheDocument()
    expect(within(rules).getByText('exclude')).toBeInTheDocument()
    expect(within(rules).getByText('on all servers')).toBeInTheDocument()

    fireEvent.click(within(rules).getByRole('button', { name: 'Revoke' }))
    await waitFor(() => {
      expect(removeMcpTagRuleMock).toHaveBeenCalledWith({ data: { ruleId: 'rule_1' } })
    })
  })

  it('needs a server before previewing an include rule', async () => {
    await renderAccessTab()
    fireEvent.click(screen.getByRole('button', { name: 'Preview matches' }))
    expect(getMcpTagRulePreviewMock).not.toHaveBeenCalled()
  })

  function choose(control: string, option: string | RegExp) {
    fireEvent.click(screen.getByRole('button', { name: control }))
    fireEvent.click(screen.getByRole('option', { name: option }))
  }

  function chooseSelect(control: string, option: string) {
    fireEvent.click(screen.getByRole('combobox', { name: control }))
    fireEvent.click(screen.getByRole('option', { name: option }))
  }

  it('saves an exclude rule for all servers', async () => {
    saveMcpTagRuleMock.mockResolvedValue({ data: tagRule })
    await renderAccessTab()
    chooseSelect('Tag rule subject type', 'User')
    choose('Tag rule subject', /Jane Admin/)
    chooseSelect('Tag rule effect', 'Exclude (deny matching tools)')
    choose('Tag rule server', 'All servers')
    fireEvent.click(screen.getByRole('button', { name: 'Save rule' }))

    await waitFor(() => {
      expect(saveMcpTagRuleMock).toHaveBeenCalledWith({
        data: {
          subject_kind: 'user',
          subject_id: 'user_1',
          server_id: undefined,
          tag: 'read_only',
          effect: 'exclude',
        },
      })
    })
  })

  it('drops a preview that returns after the form changed', async () => {
    let finish: (value: unknown) => void = () => {}
    getMcpTagRulePreviewMock.mockReturnValue(new Promise((resolve) => (finish = resolve)))
    await renderAccessTab()
    choose('Tag rule server', /GitHub/)
    fireEvent.click(screen.getByRole('button', { name: 'Preview matches' }))
    choose('Tag rule tag', /destructive/)

    const match = {
      id: 'tool_1',
      server_id: 'server_1',
      upstream_name: 'read_file',
      display_name: 'read_file',
    }
    await act(async () => finish({ data: { tools: [match], truncated: false } }))
    expect(screen.queryByTestId('mcp-tag-rule-preview')).not.toBeInTheDocument()

    // Going back to the requested tag shows the matches again.
    choose('Tag rule tag', /read_only/)
    expect(screen.getByTestId('mcp-tag-rule-preview')).toHaveTextContent('read_file')
  })

  it('clears the effective access preview after a tag rule changes', async () => {
    getMcpEffectiveAccessMock.mockResolvedValue({
      data: {
        referenced_server_count: 0,
        exposed_tool_count: 3,
        filtered_tool_count: 3,
        tools: [],
      },
    })
    removeMcpTagRuleMock.mockResolvedValue({ data: tagRule })
    await renderAccessTab()
    choose('Preview subject', /Jane Admin/)
    fireEvent.click(screen.getByRole('button', { name: 'Preview access' }))
    await screen.findByText('No callable tools for this subject.')

    fireEvent.click(
      within(screen.getByTestId('mcp-tag-rule-list')).getByRole('button', { name: 'Revoke' }),
    )
    await waitFor(() =>
      expect(screen.queryByText('No callable tools for this subject.')).not.toBeInTheDocument(),
    )
  })
})
