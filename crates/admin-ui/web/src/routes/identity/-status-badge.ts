/** Active is success, invited is warning, anything else (disabled) is neutral. Never the action colour. */
export function statusBadgeVariant(status: string): 'success' | 'warning' | 'secondary' {
  if (status === 'active') return 'success'
  if (status === 'invited') return 'warning'
  return 'secondary'
}

/** The admin team list's mapping: active is success, and any other team status is warning. */
export function teamStatusBadgeVariant(status: string): 'success' | 'warning' {
  return status === 'active' ? 'success' : 'warning'
}
