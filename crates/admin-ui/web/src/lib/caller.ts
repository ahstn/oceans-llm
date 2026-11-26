/** Who made a request or owns a session: a user, else a service account. */
export interface CallerIdentity {
  user_name?: string | null
  user_email?: string | null
  service_account_name?: string | null
}

export function callerPrimary(caller: CallerIdentity): string | null {
  return caller.user_name ?? caller.service_account_name ?? null
}

export function callerSecondary(caller: CallerIdentity): string | null {
  if (caller.user_name) {
    return caller.user_email ?? null
  }
  return caller.service_account_name ? 'service account' : null
}
