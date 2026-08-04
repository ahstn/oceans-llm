import { useEffect, useState, useTransition, type FormEvent } from 'react'
import githubIcon from '@lobehub/icons-static-svg/icons/github.svg'
import googleIcon from '@lobehub/icons-static-svg/icons/google-color.svg'
import { ViewIcon, ViewOffSlashIcon } from '@hugeicons/core-free-icons'
import { createFileRoute } from '@tanstack/react-router'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { AuthLayout } from '@/components/layout/auth-layout'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from '@/components/ui/input-group'
import { Input } from '@/components/ui/input'
import { Separator } from '@/components/ui/separator'
import { getOidcLoginOptions, loginAdminWithPassword } from '@/server/admin-data.functions'
import { postLoginAdminHref } from '@/routes/-auth-routing'
import { ssoErrorMessage } from '@/routes/-login-messages'
import { cn } from '@/lib/utils'

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
  const [showPassword, setShowPassword] = useState(false)
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
      description="Use your Oceans credentials or a supported SSO provider."
    >
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
              className="h-11"
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              required
            />
          </Field>

          <Field>
            <FieldLabel htmlFor="password">Password</FieldLabel>
            <InputGroup className="h-11">
              <InputGroupInput
                id="password"
                type={showPassword ? 'text' : 'password'}
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                required
              />
              <InputGroupAddon align="inline-end">
                <InputGroupButton
                  size="icon-xs"
                  aria-label={showPassword ? 'Hide password' : 'Show password'}
                  onClick={() => setShowPassword((visible) => !visible)}
                >
                  <AppIcon
                    icon={showPassword ? ViewOffSlashIcon : ViewIcon}
                    aria-hidden
                    data-icon="inline-start"
                  />
                </InputGroupButton>
              </InputGroupAddon>
            </InputGroup>
            <FieldDescription>Use your Oceans password.</FieldDescription>
          </Field>
        </FieldGroup>

        <Button className="w-full" size="lg" type="submit" disabled={!isHydrated || isPending}>
          {isPending ? 'Signing in…' : 'Sign in'}
        </Button>
      </form>

      {oidcProviders.data.providers.length > 0 || oauthProviders.data.providers.length > 0 ? (
        <div className="flex flex-col gap-5">
          <div className="flex items-center gap-4">
            <Separator className="flex-1" />
            <span className="text-muted-foreground text-sm">or continue with</span>
            <Separator className="flex-1" />
          </div>

          <div className="grid gap-3 sm:grid-cols-2">
            {oidcProviders.data.providers.map((provider) => (
              <Button asChild key={`oidc-${provider.key}`} size="lg" variant="outline">
                <a href={oidcStartUrl(oidcLoginOptions.startOrigin, provider.key, search.redirect)}>
                  <ProviderIcon providerKey={provider.key} />
                  {provider.label}
                </a>
              </Button>
            ))}
            {oauthProviders.data.providers.map((provider) => (
              <Button asChild key={`oauth-${provider.key}`} size="lg" variant="outline">
                <a
                  href={oauthStartUrl(oidcLoginOptions.startOrigin, provider.key, search.redirect)}
                >
                  <ProviderIcon providerKey={provider.key} />
                  {provider.label}
                </a>
              </Button>
            ))}
          </div>
        </div>
      ) : null}

      <p className="text-muted-foreground text-sm">
        First-run environment? Bootstrap access defaults to <code>admin@local</code> /{' '}
        <code>admin</code>.
      </p>
    </AuthLayout>
  )
}

function ProviderIcon({ providerKey }: { providerKey: string }) {
  const normalizedKey = providerKey.toLowerCase()
  const icon = normalizedKey.includes('google')
    ? { src: googleIcon, invert: false }
    : normalizedKey.includes('github')
      ? { src: githubIcon, invert: true }
      : null

  return icon ? (
    <img
      src={icon.src}
      alt=""
      width={16}
      height={16}
      data-icon="inline-start"
      className={cn(icon.invert && 'invert')}
      aria-hidden
    />
  ) : null
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
