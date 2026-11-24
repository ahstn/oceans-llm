import { BrandIcon } from '@/components/icons/brand-icon'

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

/**
 * The Caller column shared by request logs and agent sessions: the user's name over their email,
 * or a service account's name over `service account`.
 */
export function CallerCell({ caller }: { caller: CallerIdentity }) {
  const primary = callerPrimary(caller) ?? 'Unknown'
  const secondary = callerSecondary(caller)

  return (
    <div className="min-w-0" title={secondary ? `${primary} · ${secondary}` : primary}>
      <div className="text-foreground truncate">{primary}</div>
      {secondary ? <div className="text-muted-foreground truncate text-xs">{secondary}</div> : null}
    </div>
  )
}

/**
 * The Model column shared by request logs and agent sessions: the model with its brand icon over
 * the provider that served it. A row without a known provider shows the model alone.
 */
export function ModelCell({
  modelKey,
  modelIconKey,
  providerKey,
  providerIconKey,
}: {
  modelKey: string
  modelIconKey?: string | null
  providerKey?: string | null
  providerIconKey?: string | null
}) {
  return (
    <div className="min-w-0">
      <div className="text-foreground flex items-center gap-2 truncate">
        <BrandIcon iconKey={modelIconKey} size={16} />
        <span className="truncate">{modelKey}</span>
      </div>
      {providerKey ? (
        <div className="text-muted-foreground mt-0.5 flex items-center gap-2 truncate text-xs">
          <BrandIcon iconKey={providerIconKey} size={12} />
          <span className="truncate">{providerKey}</span>
        </div>
      ) : null}
    </div>
  )
}
