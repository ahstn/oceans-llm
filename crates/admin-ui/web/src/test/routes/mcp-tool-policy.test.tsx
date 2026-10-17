import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { McpToolView } from '@/types/api'

vi.mock('sonner', () => ({ toast: { error: vi.fn(), success: vi.fn() } }))

const saveMcpToolTagsMock = vi.fn()

vi.mock('@/server/mcp-policy-data.functions', () => ({
  saveMcpToolTags: (...args: unknown[]) => saveMcpToolTagsMock(...args),
  saveMcpToolAnnotationOverrides: vi.fn(),
}))

const hints = {
  read_only_hint: null,
  destructive_hint: null,
  idempotent_hint: null,
  open_world_hint: null,
}

const tool: McpToolView = {
  id: 'tool_1',
  server_id: 'server_1',
  upstream_name: 'read_file',
  display_name: 'read_file',
  title: null,
  description: 'Read a file',
  input_schema: { type: 'object' },
  output_schema: null,
  annotations: hints,
  annotation_overrides: hints,
  effective_annotations: hints,
  admin_tags: [],
  effective_tags: ['destructive', 'open_world'],
  approval_status: 'approved',
  approved_schema_version: 1,
  pending_schema_version: null,
  schema_hash: 'sha256:a',
  schema_version: 1,
  is_active: true,
  first_discovered_at: '2026-10-17T08:00:00Z',
  last_discovered_at: '2026-10-17T09:00:00Z',
  deactivated_at: null,
}

describe('ToolPolicyEditor drafts', () => {
  afterEach(() => cleanup())

  it('keeps each draft until its own stored value changes', async () => {
    const { ServerToolsPanel } = await import('@/routes/mcp/-components')
    const panel = (current: McpToolView) => (
      <ServerToolsPanel
        tools={[current]}
        toolsPending={false}
        toolsError={null}
        selectedToolIds={[]}
        onToggleTool={vi.fn()}
        onClearSelection={vi.fn()}
        onAddToToolset={vi.fn()}
        onToolUpdated={vi.fn()}
      />
    )
    const { rerender } = render(panel(tool))
    fireEvent.click(screen.getByRole('button', { name: 'Show read_file schema' }))
    fireEvent.change(screen.getByLabelText('Admin tags'), { target: { value: 'draft' } })

    rerender(panel({ ...tool, description: 'Read one file' }))
    expect(screen.getByLabelText('Admin tags')).toHaveValue('draft')

    // Saving overrides changes only the overrides, so the unsaved tag draft stays.
    rerender(
      panel({
        ...tool,
        annotation_overrides: { ...hints, read_only_hint: true },
      }),
    )
    expect(screen.getByLabelText('Admin tags')).toHaveValue('draft')

    rerender(
      panel({
        ...tool,
        annotation_overrides: { ...hints, read_only_hint: true },
        admin_tags: ['finance'],
      }),
    )
    expect(screen.getByLabelText('Admin tags')).toHaveValue('finance')
  })

  it('locks an editor while its save is pending', async () => {
    const { ToolPolicyEditor } = await import('@/routes/mcp/-tool-policy')
    let finish: (value: unknown) => void = () => {}
    saveMcpToolTagsMock.mockReturnValue(new Promise((resolve) => (finish = resolve)))
    const onToolUpdated = vi.fn()
    render(<ToolPolicyEditor tool={tool} onToolUpdated={onToolUpdated} />)

    fireEvent.change(screen.getByLabelText('Admin tags'), { target: { value: 'finance' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save tags' }))
    // No newer edit can be typed for the response to overwrite, and the
    // sibling editor cannot start a save that would race this one.
    expect(screen.getByLabelText('Admin tags')).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Save overrides' })).toBeDisabled()

    await act(async () => finish({ data: { tool: { ...tool, admin_tags: ['finance'] } } }))
    expect(onToolUpdated).toHaveBeenCalledOnce()
    expect(screen.getByLabelText('Admin tags')).toBeEnabled()
    expect(screen.getByRole('button', { name: 'Save overrides' })).toBeEnabled()
  })
})
