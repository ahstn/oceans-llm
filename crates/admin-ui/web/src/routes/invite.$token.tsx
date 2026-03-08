import { useState, useTransition, type FormEvent } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { completeInvitePassword, getInviteState } from '@/server/admin-data.functions'
import type { InvitationStateView } from '@/types/api'

export const Route = createFileRoute('/invite/$token')({
  loader: ({ params }) => getInviteState({ data: { token: params.token } }),
  component: InvitePage,
})

function InvitePage() {
  const { token } = Route.useParams()
  const { data } = Route.useLoaderData() as { data: InvitationStateView }
  const router = useRouter()
  const [password, setPassword] = useState('')
  const [passwordConfirmation, setPasswordConfirmation] = useState('')
  const [isComplete, setIsComplete] = useState(false)
  const [isPending, startTransition] = useTransition()

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (password !== passwordConfirmation) {
      toast.error('Passwords do not match')
      return
    }

    startTransition(async () => {
      try {
        await completeInvitePassword({ data: { token, password } })
        setIsComplete(true)
        toast.success('Password set')
        await router.navigate({
          to: '/account-ready',
          search: { mode: 'password', email: data.email ?? undefined },
        })
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Unable to set password')
      }
    })
  }

  const isValid = data.state === 'valid'

  return (
    <div className="min-h-screen bg-[radial-gradient(circle_at_top,_#1f3f73_0%,_#1c1c1c_34%)] p-4 text-neutral-100 sm:p-8">
      <div className="mx-auto flex min-h-[calc(100vh-32px)] max-w-3xl items-center justify-center">
        <Card className="w-full max-w-xl bg-[#131313]/95">
          <CardHeader>
            <CardTitle>Finish your account setup</CardTitle>
            <CardDescription>
              {data.email
                ? `Set the password for ${data.email}.`
                : 'This invitation is no longer available.'}
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            {isComplete ? (
              <Alert>
                <AlertTitle>Password set</AlertTitle>
                <AlertDescription>
                  Your account is ready. You can close this window and return to the gateway admin
                  flow.
                </AlertDescription>
              </Alert>
            ) : !isValid ? (
              <Alert>
                <AlertTitle>Invitation {data.state}</AlertTitle>
                <AlertDescription>
                  This link cannot be used anymore. Ask an administrator for a fresh onboarding URL.
                </AlertDescription>
              </Alert>
            ) : (
              <>
                <Alert>
                  <AlertTitle>Invitation is valid</AlertTitle>
                  <AlertDescription>
                    {data.expires_at
                      ? `This link expires at ${data.expires_at}.`
                      : 'Complete setup now to avoid needing a replacement invite.'}
                  </AlertDescription>
                </Alert>

                <form className="flex flex-col gap-5" onSubmit={handleSubmit}>
                  <FieldGroup>
                    <Field>
                      <FieldLabel htmlFor="password">Password</FieldLabel>
                      <Input
                        id="password"
                        type="password"
                        value={password}
                        onChange={(event) => setPassword(event.target.value)}
                        minLength={8}
                        required
                      />
                      <FieldDescription>Use at least 8 characters.</FieldDescription>
                    </Field>

                    <Field>
                      <FieldLabel htmlFor="password-confirmation">Confirm password</FieldLabel>
                      <Input
                        id="password-confirmation"
                        type="password"
                        value={passwordConfirmation}
                        onChange={(event) => setPasswordConfirmation(event.target.value)}
                        minLength={8}
                        required
                      />
                    </Field>
                  </FieldGroup>

                  <div className="flex justify-end">
                    <Button type="submit" disabled={isPending}>
                      {isPending ? 'Saving…' : 'Set password'}
                    </Button>
                  </div>
                </form>
              </>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
