import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { AlertTimeline } from '@/routes/spend-controls/-alert-timeline'
import type { BudgetAlertHistoryItemView } from '@/types/api'

const alert: BudgetAlertHistoryItemView = {
  budget_alert_id: 'alert_1',
  cadence: 'monthly',
  channel: 'email',
  created_at: '2026-10-16T02:04:09Z',
  delivery_status: 'sent',
  failure_reason: null,
  last_attempted_at: '2026-10-16T02:04:10Z',
  owner_id: 'user_1',
  owner_kind: 'user',
  owner_name: 'Gina Research Engineer',
  recipient_summary: 'gina@research.local',
  remaining_budget_usd_10000: 150_000,
  sent_at: '2026-10-16T02:04:10Z',
  spend_after_usd_10000: 1_850_000,
  spend_before_usd_10000: 1_700_000,
  threshold_bps: 8_500,
  window_end: '2026-11-01T00:00:00Z',
  window_start: '2026-10-01T00:00:00Z',
}

describe('AlertTimeline', () => {
  afterEach(cleanup)

  it('shows each alert time in UTC on one line', () => {
    render(<AlertTimeline items={[alert]} />)

    // The vertical timeline gives the date one line on small screens, so it must not wrap.
    const time = screen.getByText('16 Oct 2026, 02:04 UTC')
    expect(time).toHaveAttribute('datetime', alert.created_at)
    expect(time).toHaveClass('whitespace-nowrap')
  })
})
