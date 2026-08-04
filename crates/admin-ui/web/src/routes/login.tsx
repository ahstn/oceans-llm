import { useEffect, useState, useTransition, type FormEvent } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { AuthLayout } from '@/components/layout/auth-layout'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { getOidcLoginOptions, loginAdminWithPassword } from '@/server/admin-data.functions'
import { postLoginAdminHref } from '@/routes/-auth-routing'
import { ssoErrorMessage } from '@/routes/-login-messages'

export const Route = createFileRoute('/login')({
  validateSearch: (search: Record<string, unknown>) => ({
    redirect: typeof search.redirect === 'string' ? search.redirect : undefined,
    sso_error: typeof search.sso_error === 'string' ? search.sso_error : undefined,
  }),
  loader: async () => {
    try {
      return await getOidcLoginOptions()
    } catch {
      return {
        oidcProviders: {
          data: { providers: [] },
          meta: { generated_at: new Date().toISOString() },
        },
        oauthProviders: {
          data: { providers: [] },
          meta: { generated_at: new Date().toISOString() },
        },
        startOrigin: '',
      }
    }
  },
  component: LoginPage,
})

export function LoginPage() {
  const search = Route.useSearch()
  const oidcLoginOptions = Route.useLoaderData()
  const oidcProviders = oidcLoginOptions.oidcProviders
  const oauthProviders = oidcLoginOptions.oauthProviders
  const [email, setEmail] = useState('admin@local')
  const [password, setPassword] = useState('admin')
  const [isHydrated, setIsHydrated] = useState(false)
  const [isPending, startTransition] = useTransition()
  const ssoError = ssoErrorMessage(search.sso_error)

  useEffect(() => {
    setIsHydrated(true)
  }, [])

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
      title="Sign in"
      description="Use your Oceans credentials or an enabled SSO provider to open the browser UI."
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
            <FieldDescription>Use your Oceans password.</FieldDescription>
          </Field>
        </FieldGroup>

        <div className="flex justify-end">
          <Button type="submit" disabled={!isHydrated || isPending}>
            {isPending ? 'Signing in…' : 'Sign in'}
          </Button>
        </div>
      </form>

      {oidcProviders.data.providers.length > 0 || oauthProviders.data.providers.length > 0 ? (
        <div className="flex flex-col gap-3 border-t pt-6">
          {oidcProviders.data.providers.map((provider) => (
            <Button asChild key={`oidc-${provider.key}`} variant="outline">
              <a href={oidcStartUrl(oidcLoginOptions.startOrigin, provider.key, search.redirect)}>
                Sign in with {provider.label}
              </a>
            </Button>
          ))}
          {oauthProviders.data.providers.map((provider) => (
            <Button asChild key={`oauth-${provider.key}`} variant="outline">
              <a href={oauthStartUrl(oidcLoginOptions.startOrigin, provider.key, search.redirect)}>
                Sign in with {provider.label}
              </a>
            </Button>
          ))}
        </div>
      ) : null}
    </AuthLayout>
  )
}

function oidcStartUrl(startOrigin: string, providerKey: string, redirect: string | undefined) {
  const startPath = `/api/v1/auth/oidc/start?${new URLSearchParams({
    provider_key: providerKey,
    redirect_to: ssoRedirectTarget(redirect),
  }).toString()}`
  return startOrigin ? `${startOrigin}${startPath}` : startPath
}

function oauthStartUrl(startOrigin: string, providerKey: string, redirect: string | undefined) {
  const startPath = `/api/v1/auth/oauth/start?${new URLSearchParams({
    provider_key: providerKey,
    redirect_to: ssoRedirectTarget(redirect),
  }).toString()}`
  return startOrigin ? `${startOrigin}${startPath}` : startPath
}

function ssoRedirectTarget(redirect: string | undefined) {
  if (!redirect) return '/admin'
  if (redirect.startsWith('/admin')) return redirect
  if (redirect.startsWith('/') && !redirect.startsWith('//')) return `/admin${redirect}`
  return '/admin'
}
