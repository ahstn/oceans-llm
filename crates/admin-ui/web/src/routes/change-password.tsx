import { useState, useTransition, type FormEvent } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { changeCurrentPassword } from '@/server/admin-data.functions'

export const Route = createFileRoute('/change-password')({
  validateSearch: (search: Record<string, unknown>) => ({
    redirect: typeof search.redirect === 'string' ? search.redirect : undefined,
  }),
  component: ChangePasswordPage,
})

function ChangePasswordPage() {
  const search = Route.useSearch()
  const [currentPassword, setCurrentPassword] = useState('admin')
  const [newPassword, setNewPassword] = useState('')
  const [confirmation, setConfirmation] = useState('')
  const [isPending, startTransition] = useTransition()

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()

    if (newPassword !== confirmation) {
      toast.error('Passwords do not match')
      return
    }

    startTransition(async () => {
      try {
        await changeCurrentPassword({
          data: {
            current_password: currentPassword,
            new_password: newPassword,
          },
        })
        toast.success('Password updated')
        window.location.assign(`/admin${search.redirect ?? '/api-keys'}`)
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Unable to change password')
      }
    })
  }

  return (
    <div className="min-h-screen bg-[radial-gradient(circle_at_top,_#1f3f73_0%,_#1c1c1c_34%)] p-4 text-neutral-100 sm:p-8">
      <div className="mx-auto flex min-h-[calc(100vh-32px)] max-w-3xl items-center justify-center">
        <Card className="w-full max-w-xl bg-[#131313]/95">
          <CardHeader>
            <CardTitle>Change password</CardTitle>
            <CardDescription>
              Finish admin setup by rotating the bootstrap password.
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <Alert>
              <AlertTitle>Rotation required</AlertTitle>
              <AlertDescription>
                This admin session cannot access the control plane until the password is updated.
              </AlertDescription>
            </Alert>

            <form className="flex flex-col gap-5" onSubmit={handleSubmit}>
              <FieldGroup>
                <Field>
                  <FieldLabel htmlFor="current-password">Current password</FieldLabel>
                  <Input
                    id="current-password"
                    type="password"
                    value={currentPassword}
                    onChange={(event) => setCurrentPassword(event.target.value)}
                    required
                  />
                </Field>

                <Field>
                  <FieldLabel htmlFor="new-password">New password</FieldLabel>
                  <Input
                    id="new-password"
                    type="password"
                    value={newPassword}
                    onChange={(event) => setNewPassword(event.target.value)}
                    minLength={8}
                    required
                  />
                  <FieldDescription>Use at least 8 characters.</FieldDescription>
                </Field>

                <Field>
                  <FieldLabel htmlFor="confirm-password">Confirm new password</FieldLabel>
                  <Input
                    id="confirm-password"
                    type="password"
                    value={confirmation}
                    onChange={(event) => setConfirmation(event.target.value)}
                    minLength={8}
                    required
                  />
                </Field>
              </FieldGroup>

              <div className="flex justify-end">
                <Button type="submit" disabled={isPending}>
                  {isPending ? 'Saving…' : 'Update password'}
                </Button>
              </div>
            </form>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
