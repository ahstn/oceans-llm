import type { ComponentProps, ComponentType } from 'react'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

type Search = Record<string, unknown>
type RouteOptions = {
  validateSearch: (search: Search) => Search
  beforeLoad?: (context: { search: Search }) => void
  component: ComponentType
}
type NavigationOptions = {
  to: string
  search: Search | ((previous: Search) => Search)
  replace?: boolean
}
type ToolsetsProps = {
  selectedToolsetId: string | null
  seedToolIds: string[]
  onSelectToolset: (toolsetId: string | null) => void
  onSeedConsumed: () => void
}

const routeOptions = new Map<string, RouteOptions>()
const workspaceRoute = { useLoaderData: vi.fn(), useSearch: vi.fn() }
const toolsetsRoute = { useLoaderData: vi.fn(), useSearch: vi.fn() }
const navigateMock = vi.fn()
const redirectMock = vi.fn()
const toolsetsPropsMock = vi.fn<(props: ToolsetsProps) => void>()

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: (path: string) => (options: RouteOptions) => {
    routeOptions.set(path, options)
    return path === '/mcp/toolsets' ? toolsetsRoute : workspaceRoute
  },
  redirect: (options: unknown) => redirectMock(options),
  useRouter: () => ({ navigate: navigateMock }),
  Link: ({
    to,
    search,
    children,
    ...props
  }: ComponentProps<'a'> & { to: string; search?: Record<string, string> }) => {
    const query = new URLSearchParams(search).toString()
    return (
      <a href={query ? `${to}?${query}` : to} {...props}>
        {children}
      </a>
    )
  },
}))

vi.mock('@/server/admin-data.functions', () => ({
  getApiKeys: vi.fn(),
  getMcpGrants: vi.fn(),
  getMcpServers: vi.fn(),
  getMcpToolsets: vi.fn(),
  getRecommendedMcpServers: vi.fn(),
  getUsers: vi.fn(),
}))

vi.mock('@/routes/mcp/-servers-tab', () => ({
  ServersTab: ({ onAddToToolset }: { onAddToToolset: (toolIds: string[]) => void }) => (
    <button type="button" onClick={() => onAddToToolset(['tool_1', 'tool_2'])}>
      Add to toolset
    </button>
  ),
}))

vi.mock('@/routes/mcp/-toolsets-tab', () => ({
  ToolsetsTab: (props: ToolsetsProps) => {
    toolsetsPropsMock(props)
    return null
  },
}))

vi.mock('@/routes/mcp/-access-tab', () => ({ AccessTab: () => null }))

describe('MCP navigation', () => {
  afterEach(cleanup)

  beforeEach(() => {
    navigateMock.mockReset()
    redirectMock.mockReset()
    redirectMock.mockReturnValue(new Error('MCP redirect'))
    toolsetsPropsMock.mockClear()
    workspaceRoute.useSearch.mockReturnValue({ tab: 'servers' })
    workspaceRoute.useLoaderData.mockReturnValue({ servers: [], recommended: [] })
    toolsetsRoute.useSearch.mockReturnValue({})
    toolsetsRoute.useLoaderData.mockReturnValue({ servers: [], toolsets: [] })
  })

  it('deduplicates imported IDs and removes values that cannot be tool IDs', async () => {
    await import('@/routes/mcp/toolsets')
    const route = routeOptions.get('/mcp/toolsets')!

    expect(
      route.validateSearch({
        toolset_id: 'toolset_1',
        tool_ids: ['tool_1', 7, '', ' \t ', null, 'tool_1', 'tool_2', {}],
        tab: 'servers',
      }),
    ).toEqual({ toolset_id: 'toolset_1', tool_ids: ['tool_1', 'tool_2'] })
    expect(route.validateSearch({ toolset_id: 7, tool_ids: 'tool_1' })).toEqual({
      toolset_id: undefined,
      tool_ids: undefined,
    })
  })

  it('redirects the legacy Toolsets tab while preserving the selected set', async () => {
    await import('@/routes/mcp/index')
    const route = routeOptions.get('/mcp/')!
    const search = route.validateSearch({
      tab: 'toolsets',
      toolset_id: 'toolset_1',
      server_id: 'server_1',
    })

    expect(() => route.beforeLoad?.({ search })).toThrow('MCP redirect')
    expect(redirectMock).toHaveBeenCalledWith({
      to: '/mcp/toolsets',
      search: { toolset_id: 'toolset_1', tool_ids: undefined },
    })
  })

  it.each(['servers', 'access'])('keeps the %s section in the existing workspace', async (tab) => {
    await import('@/routes/mcp/index')
    const route = routeOptions.get('/mcp/')!

    route.beforeLoad?.({ search: route.validateSearch({ tab }) })

    expect(redirectMock).not.toHaveBeenCalled()
  })

  it('shows sibling section links and forwards the selected set and imported tools', async () => {
    toolsetsRoute.useSearch.mockReturnValue({ toolset_id: 'toolset_1', tool_ids: ['tool_1'] })
    await import('@/routes/mcp/toolsets')
    const Page = routeOptions.get('/mcp/toolsets')!.component
    render(<Page />)

    const navigation = within(screen.getByRole('navigation', { name: 'MCP sections' }))
    expect(navigation.getByRole('link', { name: 'Servers' })).toHaveAttribute(
      'href',
      '/mcp?tab=servers',
    )
    expect(navigation.getByRole('link', { name: 'Tool Sets' })).toHaveAttribute(
      'href',
      '/mcp/toolsets',
    )
    expect(navigation.getByRole('link', { name: 'Tool Sets' })).toHaveAttribute(
      'aria-current',
      'page',
    )
    expect(navigation.getByRole('link', { name: 'Access' })).toHaveAttribute(
      'href',
      '/mcp?tab=access',
    )
    expect(toolsetsPropsMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ selectedToolsetId: 'toolset_1', seedToolIds: ['tool_1'] }),
    )
  })

  it('sends a server tool selection to the dedicated Tool Sets page', async () => {
    const { McpWorkspacePage } = await import('@/routes/mcp/index')
    render(<McpWorkspacePage />)

    fireEvent.click(screen.getByRole('button', { name: 'Add to toolset' }))

    expect(navigateMock).toHaveBeenCalledWith({
      to: '/mcp/toolsets',
      search: { tool_ids: ['tool_1', 'tool_2'] },
    })
  })

  it('does not restore consumed tools or lose a target when callbacks share an old render', async () => {
    let currentSearch: Search = { tool_ids: ['tool_1'] }
    toolsetsRoute.useSearch.mockReturnValue(currentSearch)
    navigateMock.mockImplementation(({ search }: NavigationOptions) => {
      currentSearch = typeof search === 'function' ? search(currentSearch) : search
    })
    await import('@/routes/mcp/toolsets')
    const Page = routeOptions.get('/mcp/toolsets')!.component
    render(<Page />)
    const props = toolsetsPropsMock.mock.lastCall![0]

    props.onSeedConsumed()
    props.onSelectToolset('toolset_2')

    expect(currentSearch).toEqual({ toolset_id: 'toolset_2', tool_ids: undefined })
    props.onSeedConsumed()
    expect(currentSearch).toEqual({ toolset_id: 'toolset_2', tool_ids: undefined })
  })
})
