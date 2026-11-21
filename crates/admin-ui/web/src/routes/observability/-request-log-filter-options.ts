import { useEffect, useRef, useState } from 'react'

import { loadAllModels } from '@/lib/model-catalog'
import type { ModelView } from '@/types/api'

// Option lists for the request log filter pickers, kept apart from the picker component so the
// component module exports only components.

export interface FilterOption {
  value: string
  label: string
  hint?: string
}

export interface FilterOptionsState {
  status: 'idle' | 'loading' | 'ready' | 'error'
  options: FilterOption[]
  /** Loads the options again after a failure; the picker calls it when it reopens. */
  retry?: () => void
}

/** Status codes the gateway and its providers commonly return; any other code can be typed. */
export const statusCodeOptions: FilterOption[] = [
  { value: '200', label: '200', hint: 'OK' },
  { value: '400', label: '400', hint: 'Bad Request' },
  { value: '401', label: '401', hint: 'Unauthorized' },
  { value: '403', label: '403', hint: 'Forbidden' },
  { value: '404', label: '404', hint: 'Not Found' },
  { value: '408', label: '408', hint: 'Request Timeout' },
  { value: '413', label: '413', hint: 'Content Too Large' },
  { value: '422', label: '422', hint: 'Unprocessable Content' },
  { value: '429', label: '429', hint: 'Too Many Requests' },
  { value: '500', label: '500', hint: 'Internal Server Error' },
  { value: '502', label: '502', hint: 'Bad Gateway' },
  { value: '503', label: '503', hint: 'Service Unavailable' },
  { value: '504', label: '504', hint: 'Gateway Timeout' },
]

export const readyStatusCodeOptions: FilterOptionsState = {
  status: 'ready',
  options: statusCodeOptions,
}

/** Accepts any three-digit HTTP status typed into the status picker. */
export function customStatusCode(search: string) {
  const code = search.trim()
  return /^[1-5]\d\d$/.test(code) ? code : null
}

/** Keys that are no longer in the catalog still match older logs, so any typed key is allowed. */
export function customModelKey(search: string) {
  return search.trim() || null
}

/**
 * Loads the gateway model catalog the first time a model filter is shown, so the request list
 * does not fetch every model page on each filter navigation.
 */
export function useModelFilterOptions(enabled: boolean): FilterOptionsState {
  const [state, setState] = useState<FilterOptionsState>({ status: 'idle', options: [] })
  const inFlight = useRef(false)

  useEffect(() => {
    if (enabled && state.status === 'idle') startModelLoad(inFlight, setState)
    // Removing the chip forgets a failed load, so adding it again loads afresh.
    if (!enabled && state.status === 'error') setState({ status: 'idle', options: [] })
  }, [enabled, state.status])

  // A failed load is not final: reopening the picker tries again.
  const retry = () => {
    if (state.status === 'error') startModelLoad(inFlight, setState)
  }
  return { ...state, retry }
}

function startModelLoad(
  inFlight: { current: boolean },
  setState: (state: FilterOptionsState) => void,
) {
  if (inFlight.current) return
  inFlight.current = true
  setState({ status: 'loading', options: [] })
  loadAllModels()
    .then((models) => setState({ status: 'ready', options: modelFilterOptions(models) }))
    .catch(() => setState({ status: 'error', options: [] }))
    .finally(() => {
      inFlight.current = false
    })
}

function modelFilterOptions(models: ModelView[]): FilterOption[] {
  const byKey = new Map<string, FilterOption>()
  for (const model of models) {
    // `id` is the model key that requests name and logs store; `model_id` is its internal UUID.
    byKey.set(model.id, {
      value: model.id,
      label: model.id,
      hint: model.alias_of ? `alias of ${model.alias_of}` : (model.provider_label ?? undefined),
    })
  }
  return [...byKey.values()].sort((left, right) => left.label.localeCompare(right.label))
}
