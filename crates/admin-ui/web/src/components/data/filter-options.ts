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
  /** Tries a failed load again; the picker calls it each time it opens. */
  load?: () => void
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
 * on every filter navigation or before the filter is shown. A failed load is tried again when the
 * picker next opens. A load still running when the component unmounts is cancelled through the
 * signal `load` receives.
 */
export function useLoadedFilterOptions(
  enabled: boolean,
  load: (signal: AbortSignal) => Promise<FilterOption[]>,
): FilterOptionsState {
  const [state, setState] = useState<FilterOptionsState>({ status: 'idle', options: [] })
  const [attempt, setAttempt] = useState(0)
  const requested = useRef(false)
  const latestLoad = useRef(load)

  useEffect(() => {
    latestLoad.current = load
  })

  useEffect(() => {
    if (!enabled || requested.current) return
    requested.current = true
    const controller = new AbortController()
    let settled = false
    setState({ status: 'loading', options: [] })
    latestLoad
      .current(controller.signal)
      .then((options) => setState({ status: 'ready', options }))
      .catch(() => {
        if (controller.signal.aborted) return
        // Leave the next attempt to `retry`, so a failing endpoint is not called in a loop.
        requested.current = false
        setState({ status: 'error', options: [] })
      })
      .finally(() => {
        settled = true
      })
    return () => {
      if (settled) return
      // A cancelled load never finished, so a later mount (or Strict Mode's re-run) starts over.
      controller.abort()
      requested.current = false
    }
  }, [enabled, attempt])

  function retry() {
    if (state.status === 'error') setAttempt((current) => current + 1)
  }

  return { ...state, load: retry }
}

/** The gateway model catalog as filter options, loaded once the model filter is shown. */
export function useModelFilterOptions(enabled: boolean): FilterOptionsState {
  return useLoadedFilterOptions(enabled, async (signal) =>
    modelFilterOptions(await loadAllModels(signal)),
  )
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
