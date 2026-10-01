import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

import { PROFILE_RANGES, toProfileRange, type ProfileRange } from './profile-data'

const SHORT_LABEL: Record<ProfileRange, string> = { 30: '30d', 90: '90d', 365: '1y' }

export function RangeToggle({
  value,
  onChange,
}: {
  value: ProfileRange
  onChange: (range: ProfileRange) => void
}) {
  return (
    <ToggleGroup
      type="single"
      variant="outline"
      size="sm"
      value={String(value)}
      onValueChange={(next) => {
        if (next) onChange(toProfileRange(next))
      }}
      aria-label="Chart window"
    >
      {PROFILE_RANGES.map((range) => (
        <ToggleGroupItem key={range.value} value={String(range.value)} aria-label={range.label}>
          {SHORT_LABEL[range.value]}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  )
}
