import { useState, useTransition, type FormEvent } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { AuthLayout } from '@/components/layout/auth-layout'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { loginAdminWithPassword } from '@/server/admin-data.functions'

export const Route = createFileRoute('/login')({
  validateSearch: (search: Record<string, unknown>) => ({
    redirect: typeof search.redirect === 'string' ? search.redirect : undefined,
  }),
  component: LoginPage,
})

function LoginPage() {
  const search = Route.useSearch()
  const [email, setEmail] = useState('admin@local')
  const [password, setPassword] = useState('admin')
  const [isPending, startTransition] = useTransition()

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()

    startTransition(async () => {
      try {
        const response = await loginAdminWithPassword({ data: { email, password } })
        toast.success('Signed in')
        const target = response.data.must_change_password
          ? search.redirect
            ? `/admin/change-password?redirect=${encodeURIComponent(search.redirect)}`
            : '/admin/change-password'
          : `/admin${search.redirect ?? '/api-keys'}`

        window.location.assign(target)
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
    </AuthLayout>
  )
}
