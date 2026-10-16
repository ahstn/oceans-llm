import '@testing-library/jest-dom/vitest'
import { act, cleanup } from '@testing-library/react'
import { afterEach, vi } from 'vitest'

// Vitest runs without globals, so Testing Library cannot register its own `afterEach(cleanup)`.
// Unmount after every test, then give React one macrotask to run the Scheduler work that commits
// outside `act` (resolved promises, timers) leave queued. Work still queued when a file finishes
// can run after jsdom teardown and fail the run with "ReferenceError: window is not defined".
afterEach(async () => {
  cleanup()
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
