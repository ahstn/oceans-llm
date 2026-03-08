import { useState, useTransition, type FormEvent } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
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
    <div className="min-h-screen bg-[radial-gradient(circle_at_top,_#1f3f73_0%,_#1c1c1c_34%)] p-4 text-neutral-100 sm:p-8">
      <div className="mx-auto flex min-h-[calc(100vh-32px)] max-w-3xl items-center justify-center">
        <Card className="w-full max-w-xl bg-[#131313]/95">
          <CardHeader>
            <CardTitle>Admin sign in</CardTitle>
            <CardDescription>Sign in with a platform admin password session.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <Alert>
              <AlertTitle>Bootstrap access</AlertTitle>
              <AlertDescription>
                First-run environments default to <code>admin@local</code> / <code>admin</code>.
              </AlertDescription>
            </Alert>

            <form className="flex flex-col gap-5" onSubmit={handleSubmit}>
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
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
