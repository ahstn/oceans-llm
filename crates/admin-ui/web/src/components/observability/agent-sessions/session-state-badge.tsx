import { Badge } from '@/components/ui/badge'

import { humanize } from './session-labels'

const STATE_VARIANTS: Record<string, 'success' | 'warning' | 'destructive'> = {
  succeeded: 'success',
  partial: 'warning',
  failed: 'destructive',
}

/** A session lifecycle or request outcome; only an outcome takes a status colour. */
export function StateBadge({ value }: { value: string }) {
  return (
    <Badge variant={STATE_VARIANTS[value.toLowerCase()] ?? 'secondary'}>{humanize(value)}</Badge>
  )
}
