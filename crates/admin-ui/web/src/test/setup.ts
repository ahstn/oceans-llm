import '@testing-library/jest-dom/vitest'
import { act, cleanup } from '@testing-library/react'
import { afterAll, afterEach, vi } from 'vitest'

// Vitest runs without globals, so Testing Library cannot register its own `afterEach(cleanup)`.
afterEach(() => {
  cleanup()
})

// Commits outside `act` (resolved promises, timers) leave React Scheduler work queued. Run it
// before Vitest tears down jsdom, where it fails with "ReferenceError: window is not defined".
// Do this once per file: awaiting it after every test makes each test start in a later
// event-loop phase, which reorders `waitFor` timers against React renders.
afterAll(async () => {
  await act(async () => {})
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
