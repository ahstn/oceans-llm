import {
  AiBrain01Icon,
  CommandLineIcon,
  DatabaseIcon,
  Layers01Icon,
} from '@hugeicons/core-free-icons'

import { AgentHarnessIcon } from '@/components/icons/agent-harness-icon'
import { AppIcon } from '@/components/icons/app-icon'
import { IconTile } from '@/components/reui/icon-tile'
import { Card, CardContent } from '@/components/ui/card'
import { cn } from '@/lib/utils'

import {
  COMPACT_FORMATTER,
  formatProfileCost,
  NUMBER_FORMATTER,
  PERCENT_FORMATTER,
  type Preference,
  type ProfileHeadlines,
  type UsageTotals,
} from './profile-data'

function shareDetail(preference: Preference | null) {
  return preference
    ? `${PERCENT_FORMATTER.format(preference.share)} of requests`
    : 'No requests yet'
}

function cacheDetail(totals: UsageTotals) {
  return totals.cacheHitRate == null
    ? 'No cacheable input yet'
    : `${COMPACT_FORMATTER.format(totals.cacheReadTokens)} tokens served from cache`
}

/** The four headline figures, each with a one-line qualifier. */
function headlineItems({ totals, model, harness }: ProfileHeadlines) {
  return [
    {
      key: 'model',
      label: 'Favourite model',
      icon: <AppIcon icon={AiBrain01Icon} size={16} stroke={1.5} />,
      value: model?.label ?? '—',
      mono: true,
      detail: shareDetail(model),
    },
    {
      key: 'harness',
      label: 'Favourite client',
      icon:
        harness && harness.key !== 'unknown' ? (
          <AgentHarnessIcon harnessKey={harness.key} size={16} />
        ) : (
          <AppIcon icon={CommandLineIcon} size={16} stroke={1.5} />
        ),
      value: harness?.label ?? '—',
      mono: false,
      detail: shareDetail(harness),
    },
    {
      key: 'tokens',
      label: 'Total tokens',
      icon: <AppIcon icon={Layers01Icon} size={16} stroke={1.5} />,
      value: COMPACT_FORMATTER.format(totals.totalTokens),
      mono: false,
      detail: `${NUMBER_FORMATTER.format(totals.requests)} requests · ${formatProfileCost(totals.costUsd10000, totals.unpricedRequests)}`,
    },
    {
      key: 'cache',
      label: 'Cache hit rate',
      icon: <AppIcon icon={DatabaseIcon} size={16} stroke={1.5} />,
      value: totals.cacheHitRate == null ? '—' : PERCENT_FORMATTER.format(totals.cacheHitRate),
      mono: false,
      detail: cacheDetail(totals),
    },
  ]
}

/** Four headline cards with an icon tile, as on Spend Controls. */
export function HeadlineTiles({
  headlines,
  className,
}: {
  headlines: ProfileHeadlines
  className?: string
}) {
  return (
    <div data-testid="profile-headlines" className={cn('grid gap-3 sm:grid-cols-2', className)}>
      {headlineItems(headlines).map((item) => (
        <Card key={item.key} size="sm">
          <CardContent className="flex items-start gap-3">
            <IconTile variant="soft" size="sm">
              {item.icon}
            </IconTile>
            <div className="flex min-w-0 flex-col">
              <span className="text-muted-foreground text-xs">{item.label}</span>
              <span
                className={cn(
                  'truncate text-lg font-semibold tabular-nums',
                  item.mono && 'font-mono text-base leading-7',
                )}
              >
                {item.value}
              </span>
              <span className="text-muted-foreground truncate text-xs">{item.detail}</span>
            </div>
          </CardContent>
        </Card>
      ))}
    </div>
  )
}
