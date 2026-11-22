import { useEffect, useRef, useState } from 'react'

import { loadAllModels } from '@/lib/model-catalog'
import type { ModelOptionView } from '@/types/api'

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
  /** Loads options that are not loaded yet or failed before; called when the picker is used. */
  load?: () => void
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
 * Loads the gateway model catalog on first use (adding the Model chip or opening its picker), so
 * a page opened with `model_key` already in the URL does not fetch the catalog. A failed load is
 * tried again on the next use. A load still running when the toolbar unmounts is cancelled.
 */
export function useModelFilterOptions(): FilterOptionsState {
  const [state, setState] = useState<FilterOptionsState>({ status: 'idle', options: [] })
  const inFlight = useRef<AbortController | null>(null)

  useEffect(() => () => inFlight.current?.abort(), [])

  const load = () => {
    if (state.status === 'idle' || state.status === 'error') startModelLoad(inFlight, setState)
  }
  return { ...state, load }
}

function startModelLoad(
  inFlight: { current: AbortController | null },
  setState: (state: FilterOptionsState) => void,
) {
  if (inFlight.current) return
  const controller = new AbortController()
  inFlight.current = controller
  setState({ status: 'loading', options: [] })
  loadAllModels(controller.signal)
    .then((models) => setState({ status: 'ready', options: modelFilterOptions(models) }))
    .catch(() => setState({ status: 'error', options: [] }))
    .finally(() => {
      inFlight.current = null
    })
}

function modelFilterOptions(models: ModelOptionView[]): FilterOption[] {
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
