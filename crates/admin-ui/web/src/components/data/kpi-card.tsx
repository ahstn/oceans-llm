import type { ReactNode } from 'react'

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'

/** A headline figure with its label and a hint below; the figure is a skeleton while `pending`. */
export function KpiCard({
  label,
  value,
  hint,
  tone = 'default',
  pending,
}: {
  label: string
  value: string
  hint: ReactNode
  tone?: 'default' | 'warning'
  pending: boolean
}) {
  return (
    <Card size="sm">
      <CardHeader>
        <CardDescription>{label}</CardDescription>
        {pending ? (
          <Skeleton className="h-7 w-28" />
        ) : (
          <CardTitle
            className={
              tone === 'warning' ? 'text-warning text-2xl tabular-nums' : 'text-2xl tabular-nums'
            }
          >
            {value}
          </CardTitle>
        )}
      </CardHeader>
      <CardContent className="text-muted-foreground text-xs">{hint}</CardContent>
    </Card>
  )
}
