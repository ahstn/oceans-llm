import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { ToolsetConnectionDialog } from '@/components/mcp/toolset-connection-dialog'
import type { McpConnectionInfoPayload, McpToolsetView } from '@/types/api'

const toolset: McpToolsetView = {
  id: 'engineering',
  toolset_key: 'engineering-essentials',
  display_name: 'Engineering essentials',
  status: 'active',
  created_at: '2026-09-05T10:00:00Z',
  updated_at: '2026-09-05T10:00:00Z',
}

const connectionInfo: McpConnectionInfoPayload = {
  endpoint: 'https://gateway.example.com/nested/mcp',
  client_configurations: [
    {
      key: 'codex',
      label: 'Codex',
      model_ids: [],
      setup: [{ label: 'Environment', value: 'Set OCEANS_LLM_API_KEY before launching Codex.' }],
      blocks: [
        {
          label: 'Remote server',
          filename: '~/.codex/config.toml',
          content:
            '[mcp_servers.oceans]\nurl = "https://gateway.example.com/nested/mcp"\nbearer_token_env_var = "OCEANS_LLM_API_KEY"',
        },
      ],
      notes: ['This configuration was supplied by the gateway.'],
    },
  ],
}

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('ToolsetConnectionDialog', () => {
  it('loads on demand and copies the shared endpoint without adding a tool set path', async () => {
    const loadConnectionInfo = vi.fn().mockResolvedValue(connectionInfo)
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } })
    render(<ToolsetConnectionDialog toolset={toolset} loadConnectionInfo={loadConnectionInfo} />)
    expect(loadConnectionInfo).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Connection Info' }))
    expect(await screen.findByText(connectionInfo.endpoint)).toBeVisible()
    expect(
      screen.getByText(/grant Engineering essentials to the API key or its owner/),
    ).toBeVisible()
    expect(screen.getByText('Streamable HTTP')).toBeVisible()
    expect(screen.getByText(/owned by a user or service account/)).toBeVisible()
    expect(screen.getByText(/Save changes in the navigator/)).toBeVisible()
    fireEvent.click(screen.getByRole('button', { name: 'Copy MCP endpoint' }))
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(connectionInfo.endpoint))
    expect(loadConnectionInfo).toHaveBeenCalledOnce()
  })

  it('renders and copies server-owned configuration without altering its contents', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } })
    render(
      <ToolsetConnectionDialog toolset={toolset} loadConnectionInfo={async () => connectionInfo} />,
    )
    fireEvent.click(screen.getByRole('button', { name: 'Connection Info' }))
    const codex = await screen.findByRole('radio', { name: 'Codex' })
    fireEvent.click(codex)
    expect(codex).toBeChecked()
    expect(screen.getByText('Set OCEANS_LLM_API_KEY before launching Codex.')).toBeVisible()
    expect(screen.getByText('This configuration was supplied by the gateway.')).toBeVisible()
    fireEvent.click(screen.getByRole('button', { name: 'Copy ~/.codex/config.toml' }))
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        connectionInfo.client_configurations[0].blocks[0].content,
      ),
    )
    expect(screen.getByTestId('toolset-connection-panel')).toHaveClass(
      'min-w-0',
      'max-w-full',
      'overflow-hidden',
    )
    const viewport = screen.getByRole('region', { name: 'toml code' })
    expect(viewport).toHaveClass('min-w-0', 'max-w-full', 'overflow-auto')
    expect(screen.getByTestId('toolset-connection-dialog')).toHaveClass('overflow-hidden')
    expect(screen.getByTestId('toolset-connection-scroll')).toHaveClass(
      'min-h-0',
      'min-w-0',
      'max-w-full',
      'overflow-y-auto',
    )
    expect(screen.getByRole('radiogroup', { name: 'Connection setup' })).toHaveAttribute(
      'data-orientation',
      'horizontal',
    )
    const connection = screen.getByRole('radio', { name: 'Connection', exact: true })
    fireEvent.click(connection)
    expect(connection).toBeChecked()
    expect(codex).not.toBeChecked()
    expect(screen.getByText(connectionInfo.endpoint)).toBeVisible()
    fireEvent.click(connection)
    expect(connection).toBeChecked()
  })

  it('offers retry after a failed load without exposing the raw error', async () => {
    const loadConnectionInfo = vi
      .fn()
      .mockRejectedValueOnce(new Error('https://private:secret@example.com/mcp'))
      .mockResolvedValueOnce(connectionInfo)
    render(<ToolsetConnectionDialog toolset={toolset} loadConnectionInfo={loadConnectionInfo} />)
    fireEvent.click(screen.getByRole('button', { name: 'Connection Info' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Connection info could not be loaded',
    )
    expect(screen.queryByText(/private:secret/)).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Retry connection info' }))
    expect(await screen.findByText(connectionInfo.endpoint)).toBeVisible()
    expect(loadConnectionInfo).toHaveBeenCalledTimes(2)
  })

  it('ignores a late response from a previous opening', async () => {
    let resolveFirst!: (value: McpConnectionInfoPayload) => void
    const first = new Promise<McpConnectionInfoPayload>((resolve) => {
      resolveFirst = resolve
    })
    const loadConnectionInfo = vi
      .fn()
      .mockReturnValueOnce(first)
      .mockResolvedValueOnce(connectionInfo)
    render(<ToolsetConnectionDialog toolset={toolset} loadConnectionInfo={loadConnectionInfo} />)
    fireEvent.click(screen.getByRole('button', { name: 'Connection Info' }))
    expect(screen.getByRole('status')).toHaveTextContent('Loading connection info')
    fireEvent.click(screen.getByRole('button', { name: 'Close' }))
    fireEvent.click(screen.getByRole('button', { name: 'Connection Info' }))
    expect(await screen.findByText(connectionInfo.endpoint)).toBeVisible()
    await act(async () => {
      resolveFirst({ ...connectionInfo, endpoint: 'https://stale.example.com/mcp' })
      await first
    })
    expect(screen.queryByText('https://stale.example.com/mcp')).not.toBeInTheDocument()
    expect(screen.getByText(connectionInfo.endpoint)).toBeVisible()
  })

  it('explains a disabled set without implying the shared connection is disabled', async () => {
    render(
      <ToolsetConnectionDialog
        toolset={{ ...toolset, status: 'disabled' }}
        loadConnectionInfo={async () => connectionInfo}
      />,
    )
    fireEvent.click(screen.getByRole('button', { name: 'Connection Info' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('This tool set is disabled')
    expect(screen.getByText(/Other grants can still provide tools/)).toBeVisible()
    expect(screen.getByRole('button', { name: 'Copy MCP endpoint' })).toBeEnabled()
  })
})
