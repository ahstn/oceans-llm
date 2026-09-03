import { useState, useTransition } from 'react'
import { toast } from 'sonner'

import { getErrorMessage } from '@/lib/errors'
import { CURRENCY_FORMATTER, formatUsd10000 } from '@/lib/format'
import { getSpendUsageReport } from '@/server/admin-data.functions'
import type { SpendOwnerKind, SpendReportView } from '@/types/api'

export type WindowDays = 7 | 30

export { CURRENCY_FORMATTER }

export const NUMBER_FORMATTER = new Intl.NumberFormat('en-US')

export const PERCENT_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'percent',
  maximumFractionDigits: 1,
})

export const formatUsd = formatUsd10000

export function formatCount(value: number | null | undefined) {
  return value == null ? 'Unavailable' : NUMBER_FORMATTER.format(value)
}

export function formatShare(part: number, whole: number) {
  return whole > 0 ? PERCENT_FORMATTER.format(part / whole) : '—'
}

export function formatDay(value: string) {
  return new Date(value).toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric',
    timeZone: 'UTC',
  })
}

/** Requests the report saw in the window, priced or not. */
export function totalRequests(totals: SpendReportView['totals']) {
  return (
    totals.priced_request_count + totals.unpriced_request_count + totals.usage_missing_request_count
  )
}

/** Share of requests that carried a price. */
export function pricingCoverage(totals: SpendReportView['totals']) {
  const total = totalRequests(totals)
  return total > 0 ? totals.priced_request_count / total : 1
}

export function useSpendReport(initial: SpendReportView) {
  const [report, setReport] = useState<SpendReportView>(initial)
  const [windowDays, setWindowDays] = useState<WindowDays>(initial.window_days === 30 ? 30 : 7)
  const [ownerKind, setOwnerKind] = useState<SpendOwnerKind>(
    (initial.owner_kind as SpendOwnerKind) ?? 'all',
  )
  const [isPending, startTransition] = useTransition()

  // Filters commit with the response so the controls always describe the rendered report.
  function load(nextDays: WindowDays, nextOwnerKind: SpendOwnerKind) {
    startTransition(async () => {
      try {
        const response = await getSpendUsageReport({
          data: { days: nextDays, owner_kind: nextOwnerKind },
        })
        setReport(response.data)
        setWindowDays(nextDays)
        setOwnerKind(nextOwnerKind)
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  return {
    report,
    windowDays,
    ownerKind,
    isPending,
    setWindowDays: (days: WindowDays) => load(days, ownerKind),
    setOwnerKind: (kind: SpendOwnerKind) => load(windowDays, kind),
    refresh: () => load(windowDays, ownerKind),
  }
}

export function toWindowDays(value: string): WindowDays {
  return value === '30' ? 30 : 7
}

export type FocusExportTarget = {
  ownerKind: SpendOwnerKind
  currentUserOnly: boolean
  /** Browser-facing gateway origin, so the download hits the gateway even on the raw UI port. */
  origin: string
}

export function downloadFocusRange(windowDays: WindowDays, target: FocusExportTarget) {
  const end = utcDateAtDayOffset(0)
  const start = utcDateAtDayOffset(-(windowDays - 1))
  const params = new URLSearchParams({ start, end, granularity: 'daily' })
  navigateToFocusExport(params, target)
}

export function downloadFocusDay(day: string, target: FocusExportTarget) {
  if (!day) {
    toast.error('Choose a day to export')
    return
  }
  if (!/^\d{4}-\d{2}-\d{2}$/.test(day)) {
    toast.error('Day must be in YYYY-MM-DD format')
    return
  }
  const params = new URLSearchParams({ day, granularity: 'daily' })
  navigateToFocusExport(params, target)
}

function navigateToFocusExport(
  params: URLSearchParams,
  { ownerKind, currentUserOnly, origin }: FocusExportTarget,
) {
  if (!currentUserOnly && ownerKind !== 'all') {
    params.set('owner_kind', ownerKind)
  }
  const path = currentUserOnly ? '/api/v1/me/spend/focus.csv' : '/api/v1/admin/spend/focus.csv'
  window.location.assign(`${origin}${path}?${params.toString()}`)
}

function utcDateAtDayOffset(dayOffset: number) {
  const date = new Date()
  date.setUTCDate(date.getUTCDate() + dayOffset)
  return formatUtcDate(date)
}

export function formatUtcDate(date: Date) {
  const year = date.getUTCFullYear()
  const month = String(date.getUTCMonth() + 1).padStart(2, '0')
  const day = String(date.getUTCDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}
