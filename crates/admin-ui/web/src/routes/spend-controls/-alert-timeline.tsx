import {
  Timeline,
  TimelineContent,
  TimelineDate,
  TimelineHeader,
  TimelineIndicator,
  TimelineItem,
  TimelineSeparator,
  TimelineTitle,
} from '@/components/reui/timeline'
import { Badge } from '@/components/ui/badge'
import { formatUsd10000 } from '@/lib/format'
import type { BudgetAlertHistoryItemView } from '@/types/api'

const DELIVERY_BADGE_VARIANT: Record<string, 'success' | 'warning' | 'destructive'> = {
  sent: 'success',
  pending: 'warning',
  failed: 'destructive',
}

const DELIVERY_INDICATOR_CLASS: Record<string, string> = {
  sent: 'border-[var(--color-success)] bg-[var(--color-success-soft)]',
  pending: 'border-[var(--color-warning)] bg-[var(--color-warning-soft)]',
  failed: 'border-destructive bg-destructive/10',
}

export function AlertTimeline({ items }: { items: BudgetAlertHistoryItemView[] }) {
  if (items.length === 0) {
    return (
      <p className="text-muted-foreground py-4 text-sm">No budget alerts have been recorded yet.</p>
    )
  }
  return (
    <Timeline value={0} className="pl-6">
      {items.map((alert, index) => (
        <TimelineItem key={alert.budget_alert_id} step={index + 1}>
          <TimelineHeader>
            <TimelineSeparator />
            <TimelineDate dateTime={alert.created_at}>
              {new Date(alert.created_at).toLocaleString()}
            </TimelineDate>
            <TimelineTitle className="flex flex-wrap items-center gap-2">
              <span className="truncate">{alert.owner_name}</span>
              <Badge variant="outline">
                {alert.owner_kind === 'service_account' ? 'service account' : alert.owner_kind}
              </Badge>
              <Badge variant={DELIVERY_BADGE_VARIANT[alert.delivery_status] ?? 'default'}>
                {alert.delivery_status}
              </Badge>
            </TimelineTitle>
            <TimelineIndicator className={DELIVERY_INDICATOR_CLASS[alert.delivery_status] ?? ''} />
          </TimelineHeader>
          <TimelineContent>
            Crossed {alert.threshold_bps / 100}% of the {alert.cadence} budget with{' '}
            {formatUsd10000(alert.remaining_budget_usd_10000)} remaining. Notified{' '}
            {alert.recipient_summary}.
            {alert.failure_reason ? (
              <span className="text-destructive mt-1 block text-xs">{alert.failure_reason}</span>
            ) : null}
          </TimelineContent>
        </TimelineItem>
      ))}
    </Timeline>
  )
}
