import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { formatCompactCount, formatCount, isCompactCount } from '@/lib/format'

/**
 * A table count: grouped below 100,000 (`9,812`), compact from there (`34.4M`) with the exact count
 * in a tooltip. `unit` labels the tooltip, e.g. `34,356,226 tokens`.
 */
export function CompactCount({ value, unit }: { value: number | null | undefined; unit?: string }) {
  if (!isCompactCount(value)) {
    return <>{formatCount(value)}</>
  }

  const exact = unit ? `${formatCount(value)} ${unit}` : formatCount(value)
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        {/* Focusable so keyboard users can reach the exact count too. */}
        <span
          tabIndex={0}
          className="focus-visible:ring-ring/50 cursor-default rounded-sm outline-none focus-visible:ring-3"
          data-testid="compact-count"
        >
          {formatCompactCount(value)}
        </span>
      </TooltipTrigger>
      <TooltipContent>{exact}</TooltipContent>
    </Tooltip>
  )
}
