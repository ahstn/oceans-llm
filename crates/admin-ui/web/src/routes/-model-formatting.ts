import type { ModelView } from '@/types/api'

const CURRENCY_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 4,
})

const COMPACT_NUMBER_FORMATTER = new Intl.NumberFormat('en-US', {
  maximumFractionDigits: 2,
})

export function providerTypeLabel(model: ModelView) {
  return model.provider_label ?? model.provider_key ?? 'Unresolved'
}

export function formatCost(value: number | null | undefined) {
  if (value == null) {
    return '—'
  }

  return CURRENCY_FORMATTER.format(value / 10_000)
}

export function formatWindow(value: number | null | undefined) {
  if (value == null) {
    return '—'
  }

  if (value >= 1_000_000) {
    return `${COMPACT_NUMBER_FORMATTER.format(value / 1_000_000)}M`
  }

  if (value >= 1_000) {
    return `${COMPACT_NUMBER_FORMATTER.format(value / 1_000)}k`
  }

  return String(value)
}
