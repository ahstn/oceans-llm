// A DORA measure as a tile: its value and where it sits in the four performance bands. Shared by
// the live Delivery tab and its concept mockup, so the two can't drift apart.

import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader } from '@/components/ui/card'

import type { DoraBand } from './delivery-bands'
import type { Readiness } from './format'
import { ReadinessBadge } from './parts'

const BANDS: DoraBand[] = ['Low', 'Medium', 'High', 'Elite']

export function DoraTile({
  label,
  value,
  band,
  note,
  readiness,
  readinessNote,
}: {
  label: string
  value: string
  /** Omitted when there is no value to place, which leaves the meter empty. */
  band?: DoraBand
  note: string
  /** Omit for a measure Oceans has; a proxy shows as an icon beside the label. */
  readiness?: Readiness
  readinessNote?: string
}) {
  return (
    <Card size="sm" className="min-w-0">
      <CardHeader>
        <CardDescription className="flex flex-col items-start gap-1.5">
          <span className="flex items-center gap-1.5">
            {label}
            {readiness === 'proxy' ? (
              <ReadinessBadge level="proxy" note={readinessNote} compact />
            ) : null}
          </span>
          {readiness && readiness !== 'proxy' ? (
            <ReadinessBadge level={readiness} note={readinessNote} />
          ) : null}
        </CardDescription>
        <p className="text-2xl font-medium tracking-tight tabular-nums">{value}</p>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        <DoraBandMeter band={band} />
        <span className="text-muted-foreground text-xs">{note}</span>
      </CardContent>
    </Card>
  )
}

/** The four bands with the current one filled. Neutral: a band is a position, not a verdict. */
function DoraBandMeter({ band }: { band?: DoraBand }) {
  const index = band ? BANDS.indexOf(band) : -1
  return (
    <div className="flex items-center gap-2">
      <div className="flex flex-1 gap-0.5" aria-hidden>
        {BANDS.map((entry, position) => (
          <span
            key={entry}
            className={
              position <= index
                ? 'bg-chart-hero h-1.5 flex-1 rounded-full'
                : 'bg-muted h-1.5 flex-1 rounded-full'
            }
          />
        ))}
      </div>
      {band ? <Badge variant="outline">{band}</Badge> : null}
    </div>
  )
}
