/**
 * Spread onto a component to pin a state without interaction. Kept out of showcase.tsx so that
 * module exports only components, which Fast Refresh needs to preserve state.
 */
export function previewState(state: string) {
  if (state === 'disabled') return { disabled: true }
  if (state === 'hover' || state === 'focus' || state === 'active') {
    return { 'data-preview': state }
  }
  return {}
}
