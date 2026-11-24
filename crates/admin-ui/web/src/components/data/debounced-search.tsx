import { useEffect, useRef, useState } from 'react'
import { Search01Icon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'

const searchDebounceMs = 250

interface DebouncedSearchProps {
  /** The applied search, usually from the URL. */
  value: string
  label: string
  placeholder: string
  /** Called with the typed text once typing pauses, and only when it differs from `value`. */
  onSearch: (query: string) => void
}

/**
 * A toolbar search box that applies its text after a short pause, so each keystroke does not
 * navigate. Request logs and agent sessions share it for their model or user search.
 */
export function DebouncedSearch({ value, label, placeholder, onSearch }: DebouncedSearchProps) {
  const [query, setQuery] = useState(value)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  // The debounce timer outlives the render that scheduled it, so read the latest props from refs.
  const latest = useRef({ value, onSearch })
  // The search last sent to `onSearch` whose navigation has not reached `value` yet. Typing is
  // compared with it rather than the URL, so clearing a search that is still loading is sent too.
  const submitted = useRef<string | null>(null)

  useEffect(() => {
    latest.current = { value, onSearch }
  })

  useEffect(() => {
    // A URL from an earlier search that landed after a newer one was sent is stale: keep the box.
    if (submitted.current !== null && submitted.current !== value) {
      return
    }
    submitted.current = null
    // Only adopt URL changes (Back/Forward, Clear) when no keystrokes are waiting to be sent.
    if (timer.current === null) {
      setQuery(value)
    }
  }, [value])

  useEffect(() => () => clearTimeout(timer.current ?? undefined), [])

  function updateQuery(next: string) {
    setQuery(next)
    clearTimeout(timer.current ?? undefined)
    timer.current = setTimeout(() => {
      timer.current = null
      const search = next.trim()
      if (search !== (submitted.current ?? latest.current.value)) {
        submitted.current = search
        latest.current.onSearch(next)
      }
    }, searchDebounceMs)
  }

  return (
    <InputGroup className="w-full sm:max-w-xs">
      <InputGroupInput
        type="search"
        aria-label={label}
        placeholder={placeholder}
        value={query}
        onChange={(event) => updateQuery(event.target.value)}
      />
      <InputGroupAddon>
        <AppIcon icon={Search01Icon} aria-hidden />
      </InputGroupAddon>
    </InputGroup>
  )
}
