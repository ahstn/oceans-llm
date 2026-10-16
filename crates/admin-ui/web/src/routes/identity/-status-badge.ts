/** Active is success, invited is warning, anything else (disabled) is neutral. Never the action colour. */
export function statusBadgeVariant(status: string): 'success' | 'warning' | 'secondary' {
  if (status === 'active') return 'success'
  if (status === 'invited') return 'warning'
  return 'secondary'
}
