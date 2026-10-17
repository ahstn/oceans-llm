import '@testing-library/jest-dom/vitest'
import { createRequire } from 'node:module'
import { act, cleanup } from '@testing-library/react'
import { afterAll, afterEach, vi } from 'vitest'

// React DOM's own copy of the Scheduler, resolved the way React DOM resolves it.
const requireFromReactDom = createRequire(createRequire(import.meta.url).resolve('react-dom'))
const scheduler = requireFromReactDom('scheduler') as {
  unstable_IdlePriority: number
  unstable_scheduleCallback: (priority: number, callback: () => void) => unknown
}

// Vitest runs without globals, so Testing Library cannot register its own `afterEach(cleanup)`.
afterEach(() => {
  cleanup()
})

// Commits outside `act` (resolved promises, timers) leave React Scheduler work queued. Let it all
// run before Vitest tears down jsdom, where it fails with "ReferenceError: window is not defined".
// An idle-priority task runs only after every other queued task, including ones that yielded and
// re-queued themselves; `act` flushes whatever React schedules in the meantime.
// Do this once per file: awaiting it after every test makes each test start in a later
// event-loop phase, which reorders `waitFor` timers against React renders.
afterAll(async () => {
  await act(
    () =>
      new Promise<void>((resolve) => {
        scheduler.unstable_scheduleCallback(scheduler.unstable_IdlePriority, () => resolve())
      }),
  )
})

if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = vi.fn()
}

if (!window.matchMedia) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }))
}
