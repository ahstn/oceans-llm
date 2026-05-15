import { useState, useTransition, type FormEvent } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { AuthLayout } from '@/components/layout/auth-layout'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { getOidcProviders, loginAdminWithPassword } from '@/server/admin-data.functions'
import { postLoginAdminHref } from '@/routes/-auth-routing'

export const Route = createFileRoute('/login')({
  validateSearch: (search: Record<string, unknown>) => ({
    redirect: typeof search.redirect === 'string' ? search.redirect : undefined,
    sso_error: typeof search.sso_error === 'string' ? search.sso_error : undefined,
  }),
  loader: async () => {
    try {
      return await getOidcProviders()
    } catch {
      return {
        data: { providers: [] },
        meta: { generated_at: new Date().toISOString() },
      }
    }
  },
  component: LoginPage,
})

function LoginPage() {
  const search = Route.useSearch()
  const oidcProviders = Route.useLoaderData()
  const [email, setEmail] = useState('admin@local')
  const [password, setPassword] = useState('admin')
  const [isPending, startTransition] = useTransition()
  const ssoError = ssoErrorMessage(search.sso_error)

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()

    startTransition(async () => {
      try {
        const response = await loginAdminWithPassword({ data: { email, password } })
        toast.success('Signed in')
        window.location.assign(postLoginAdminHref(response.data, search.redirect))
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Unable to sign in')
      }
    })
  }

  return (
    <AuthLayout
      title="Admin sign in"
      description="Use the bootstrap platform-admin credentials or your rotated admin password to enter the control plane."
    >
      <Alert>
        <AlertTitle>Bootstrap access</AlertTitle>
        <AlertDescription>
          First-run environments default to <code>admin@local</code> / <code>admin</code>.
        </AlertDescription>
      </Alert>

      {ssoError ? (
        <Alert variant="destructive">
          <AlertTitle>SSO sign in failed</AlertTitle>
          <AlertDescription>{ssoError}</AlertDescription>
        </Alert>
      ) : null}

      <form className="flex flex-col gap-6" onSubmit={handleSubmit}>
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="email">Email</FieldLabel>
            <Input
              id="email"
              type="email"
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              required
            />
          </Field>

          <Field>
            <FieldLabel htmlFor="password">Password</FieldLabel>
            <Input
              id="password"
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              required
            />
            <FieldDescription>Use the configured bootstrap admin password.</FieldDescription>
          </Field>
        </FieldGroup>

        <div className="flex justify-end">
          <Button type="submit" disabled={isPending}>
            {isPending ? 'Signing in…' : 'Sign in'}
          </Button>
        </div>
      </form>

      {oidcProviders.data.providers.length > 0 ? (
        <div className="flex flex-col gap-3 border-t pt-6">
          {oidcProviders.data.providers.map((provider) => (
            <Button asChild key={provider.key} variant="outline">
              <a href={oidcStartUrl(provider.key, search.redirect)}>
                Sign in with {provider.label}
              </a>
            </Button>
          ))}
        </div>
      ) : null}
    </AuthLayout>
  )
}

function oidcStartUrl(providerKey: string, redirect: string | undefined) {
  return `/api/v1/auth/oidc/start?${new URLSearchParams({
    provider_key: providerKey,
    redirect_to: redirect ?? '/admin',
  }).toString()}`
}

function ssoErrorMessage(code: string | undefined) {
  switch (code) {
    case 'access_denied':
    case 'denied':
      return 'Access was denied for this SSO account.'
    case 'unmatched_identity':
      return 'This SSO account is not allowed to sign in.'
    case 'state_expired':
      return 'The SSO sign-in request expired. Start sign-in again.'
    case 'state_invalid':
      return 'The SSO sign-in request could not be verified. Start sign-in again.'
    case 'provider_failure':
      return 'The identity provider did not complete sign-in.'
    case 'identity_conflict':
      return 'A password account already exists for this email address.'
    default:
      return undefined
  }
}
