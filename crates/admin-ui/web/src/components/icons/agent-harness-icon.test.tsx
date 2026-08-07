import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { AgentHarnessLabel } from '@/components/icons/agent-harness-icon'

describe('AgentHarnessLabel', () => {
  it.each(['OpenCode', 'Pi', 'Claude Code', 'Codex'])('renders the %s harness icon', (label) => {
    const { container } = render(<AgentHarnessLabel harnessKey={label}>{label}</AgentHarnessLabel>)
    const icon = container.querySelector<HTMLElement>('[data-agent-harness-icon]')

    expect(icon).toBeInTheDocument()
    expect(icon).toHaveAttribute('style', expect.stringContaining('mask-image: url('))
    expect(icon).toHaveAttribute('style', expect.stringContaining('mask-position: center'))
    expect(icon).toHaveAttribute('style', expect.stringContaining('mask-repeat: no-repeat'))
    expect(icon).toHaveAttribute('style', expect.stringContaining('mask-size: contain'))
    expect(icon).toHaveAttribute('style', expect.stringContaining('background-color: currentcolor'))
  })

  it('keeps unknown harness labels text-only', () => {
    const { container, getByText } = render(
      <AgentHarnessLabel harnessKey="custom-client">Custom client</AgentHarnessLabel>,
    )

    expect(getByText('Custom client')).toBeInTheDocument()
    expect(container.querySelector('[data-agent-harness-icon]')).not.toBeInTheDocument()
  })
})
