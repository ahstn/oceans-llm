import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { AgentHarnessLabel } from '@/components/icons/agent-harness-icon'

describe('AgentHarnessLabel', () => {
  it.each(['OpenCode', 'Pi', 'Claude Code', 'Codex', 'Mastra'])(
    'renders the %s harness icon',
    (label) => {
      const { container } = render(
        <AgentHarnessLabel harnessKey={label}>{label}</AgentHarnessLabel>,
      )
      const icon = container.querySelector<HTMLElement>('[data-agent-harness-icon]')

      expect(icon).toBeInTheDocument()
      expect(icon?.style.getPropertyValue('-webkit-mask-image')).toContain('url(')
      expect(icon?.style.getPropertyValue('-webkit-mask-position')).toBe('center')
      expect(icon?.style.getPropertyValue('-webkit-mask-repeat')).toBe('no-repeat')
      expect(icon?.style.getPropertyValue('-webkit-mask-size')).toBe('contain')
      expect(icon?.style.getPropertyValue('mask-image')).toContain('url(')
      expect(icon?.style.getPropertyValue('mask-position')).toBe('center')
      expect(icon?.style.getPropertyValue('mask-repeat')).toBe('no-repeat')
      expect(icon?.style.getPropertyValue('mask-size')).toBe('contain')
      expect(icon?.style.getPropertyValue('background-color')).toBe('currentcolor')
    },
  )

  it('keeps Oh My Pi text-only instead of using the Pi icon', () => {
    const { container, getByText } = render(
      <AgentHarnessLabel harnessKey="oh_my_pi">Oh My Pi</AgentHarnessLabel>,
    )

    expect(getByText('Oh My Pi')).toBeInTheDocument()
    expect(container.querySelector('[data-agent-harness-icon]')).not.toBeInTheDocument()
  })

  it('keeps unknown harness labels text-only', () => {
    const { container, getByText } = render(
      <AgentHarnessLabel harnessKey="custom-client">Custom client</AgentHarnessLabel>,
    )

    expect(getByText('Custom client')).toBeInTheDocument()
    expect(container.querySelector('[data-agent-harness-icon]')).not.toBeInTheDocument()
  })
})
