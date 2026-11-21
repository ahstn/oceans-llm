import { useEffect, useRef, useState } from 'react'

import { loadAllModels } from '@/lib/model-catalog'
import type { ModelView } from '@/types/api'

// Option lists and loaders for `FilterValuePicker`, kept apart from the component so the
// component module exports only components.

export interface FilterOption {
  value: string
  label: string
  hint?: string
}

export interface FilterOptionsState {
  status: 'idle' | 'loading' | 'ready' | 'error'
  options: FilterOption[]
}

/** Options that are known up front, such as an enum's values. */
export function readyFilterOptions(options: FilterOption[]): FilterOptionsState {
  return { status: 'ready', options }
}

/** Accepts any non-empty typed value, for keys that older data may hold but the list lacks. */
export function anyTypedValue(search: string) {
  return search.trim() || null
}

/**
 * Loads a filter's options the first time `enabled` is true, so a list page does not fetch them
 * on every filter navigation or before the filter is shown.
 */
export function useLoadedFilterOptions(
  enabled: boolean,
  load: () => Promise<FilterOption[]>,
): FilterOptionsState {
  const [state, setState] = useState<FilterOptionsState>({ status: 'idle', options: [] })
  const requested = useRef(false)
  const latestLoad = useRef(load)

  useEffect(() => {
    latestLoad.current = load
  })

  useEffect(() => {
    if (!enabled || requested.current) return
    requested.current = true
    setState({ status: 'loading', options: [] })
    latestLoad
      .current()
      .then((options) => setState({ status: 'ready', options }))
      .catch(() => setState({ status: 'error', options: [] }))
  }, [enabled])

  return state
}

/** The gateway model catalog as filter options, loaded once the model filter is shown. */
export function useModelFilterOptions(enabled: boolean): FilterOptionsState {
  return useLoadedFilterOptions(enabled, async () => modelFilterOptions(await loadAllModels()))
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
