import { useState, useTransition, type FormEvent } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { AuthLayout } from '@/components/layout/auth-layout'
import { Button } from '@/components/ui/button'
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
    <AuthLayout
      title="Change password"
      description="Rotate the bootstrap password before entering the rest of the control plane."
    >
      <Alert>
        <AlertTitle>Rotation required</AlertTitle>
        <AlertDescription>
          This admin session cannot access the control plane until the password is updated.
        </AlertDescription>
      </Alert>

      <form className="flex flex-col gap-6" onSubmit={handleSubmit}>
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
    </AuthLayout>
  )
}
