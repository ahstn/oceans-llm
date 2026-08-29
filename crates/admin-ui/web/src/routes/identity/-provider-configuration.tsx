import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import type { IdentityUsersPayload } from '@/types/api'

type CopilotUserProviders = IdentityUsersPayload['copilot_user_providers']

const dateTimeFormatter = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'short',
})

interface ProviderConfigurationProps {
  userId: string
  providers: CopilotUserProviders
  tokens: Record<string, string>
  isPending: boolean
  onTokenChange: (providerKey: string, token: string) => void
  onSave: (providerKey: string) => void
  onRemove: (providerKey: string) => void
}

export function ProviderConfiguration({
  userId,
  providers,
  tokens,
  isPending,
  onTokenChange,
  onSave,
  onRemove,
}: ProviderConfigurationProps) {
  return (
    <div className="flex flex-col gap-5">
      <div>
        <h3 className="text-sm font-semibold text-[var(--color-text)]">
          GitHub Copilot user tokens
        </h3>
        <p className="mt-1 text-sm text-[var(--color-text-muted)]">
          Each token is encrypted at rest and used only for requests made with this user&apos;s
          gateway API keys.
        </p>
      </div>

      <Alert>
        <AlertTitle>Get a token with GitHub CLI</AlertTitle>
        <AlertDescription className="flex flex-col gap-2">
          <p>
            Direct Copilot authentication needs no extra GitHub OAuth scope. If GitHub CLI asks you
            to refresh its authorization, run:
          </p>
          <code className="block overflow-x-auto rounded bg-[var(--color-surface-muted)] px-3 py-2 text-xs text-[var(--color-text)]">
            gh auth refresh --hostname github.com
          </code>
          <p>
            Then copy the token with <code>gh auth token</code>. The user must have an active GitHub
            Copilot entitlement.
          </p>
        </AlertDescription>
      </Alert>

      {providers.length === 0 ? (
        <Alert>
          <AlertTitle>No user-token Copilot provider configured</AlertTitle>
          <AlertDescription>
            Add a GitHub Copilot provider with <code>auth.mode: github_user</code> to the gateway
            configuration.
          </AlertDescription>
        </Alert>
      ) : (
        providers.map((provider) => {
          const status = provider.credentials.find((credential) => credential.user_id === userId)
          const token = tokens[provider.provider_key] ?? ''
          return (
            <form
              key={provider.provider_key}
              className="flex flex-col gap-4 border-t border-[color:var(--color-border)] pt-5 first:border-t-0 first:pt-0"
              onSubmit={(event) => {
                event.preventDefault()
                if (!isPending && token.trim().length > 0) {
                  onSave(provider.provider_key)
                }
              }}
            >
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div>
                  <h4 className="font-mono text-sm font-semibold text-[var(--color-text)]">
                    {provider.provider_key}
                  </h4>
                  <p className="mt-1 text-xs text-[var(--color-text-muted)]">
                    {status?.updated_at
                      ? `Updated ${formatDateTime(status.updated_at)}`
                      : 'No token has been stored.'}
                  </p>
                </div>
                <Badge variant={status?.configured ? 'success' : 'default'}>
                  {status?.configured ? 'Configured' : 'Not configured'}
                </Badge>
              </div>

              <Field>
                <FieldLabel htmlFor={`provider-token-${provider.provider_key}`}>
                  GitHub token
                </FieldLabel>
                <Input
                  id={`provider-token-${provider.provider_key}`}
                  type="password"
                  autoComplete="off"
                  value={token}
                  placeholder={
                    status?.configured
                      ? 'Enter a new token to replace the stored token'
                      : 'Paste the output of gh auth token'
                  }
                  onChange={(event) => onTokenChange(provider.provider_key, event.target.value)}
                />
                <FieldDescription>
                  The stored value is never returned to the browser after save.
                </FieldDescription>
              </Field>

              <div className="flex flex-wrap gap-2">
                <Button type="submit" disabled={isPending || token.trim().length === 0}>
                  {isPending ? 'Saving…' : 'Save token'}
                </Button>
                {status?.configured ? (
                  <Button
                    type="button"
                    variant="secondary"
                    onClick={() => onRemove(provider.provider_key)}
                    disabled={isPending}
                  >
                    Remove token
                  </Button>
                ) : null}
              </div>
            </form>
          )
        })
      )}
    </div>
  )
}

function formatDateTime(value: string) {
  return dateTimeFormatter.format(new Date(value))
}
