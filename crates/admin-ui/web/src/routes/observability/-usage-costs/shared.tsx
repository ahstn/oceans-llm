import { useState, useTransition } from 'react'
import { toast } from 'sonner'

import { getSpendUsageReport } from '@/server/admin-data.functions'
import type { SpendOwnerKind, SpendReportView } from '@/types/api'

export type WindowDays = 7 | 30

export const CURRENCY_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
})

export const NUMBER_FORMATTER = new Intl.NumberFormat('en-US')

export const PERCENT_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'percent',
  maximumFractionDigits: 1,
})

export function formatUsd(amountUsd10000: number) {
  return CURRENCY_FORMATTER.format(amountUsd10000 / 10_000)
}

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

  function load(nextDays: WindowDays, nextOwnerKind: SpendOwnerKind) {
    setWindowDays(nextDays)
    setOwnerKind(nextOwnerKind)
    startTransition(async () => {
      try {
        const response = await getSpendUsageReport({
          data: { days: nextDays, owner_kind: nextOwnerKind },
        })
        setReport(response.data)
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

function getErrorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }
  return 'Request failed'
}

export function downloadFocusRange(
  windowDays: WindowDays,
  ownerKind: SpendOwnerKind,
  currentUserOnly: boolean,
) {
  const end = utcDateAtDayOffset(0)
  const start = utcDateAtDayOffset(-(windowDays - 1))
  const params = new URLSearchParams({ start, end, granularity: 'daily' })
  navigateToFocusExport(params, ownerKind, currentUserOnly)
}

export function downloadFocusDay(day: string, ownerKind: SpendOwnerKind, currentUserOnly: boolean) {
  if (!day) {
    toast.error('Choose a day to export')
    return
  }
  if (!/^\d{4}-\d{2}-\d{2}$/.test(day)) {
    toast.error('Day must be in YYYY-MM-DD format')
    return
  }
  const params = new URLSearchParams({ day, granularity: 'daily' })
  navigateToFocusExport(params, ownerKind, currentUserOnly)
}

function navigateToFocusExport(
  params: URLSearchParams,
  ownerKind: SpendOwnerKind,
  currentUserOnly: boolean,
) {
  if (!currentUserOnly && ownerKind !== 'all') {
    params.set('owner_kind', ownerKind)
  }
  const path = currentUserOnly ? '/api/v1/me/spend/focus.csv' : '/api/v1/admin/spend/focus.csv'
  window.location.assign(`${path}?${params.toString()}`)
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
