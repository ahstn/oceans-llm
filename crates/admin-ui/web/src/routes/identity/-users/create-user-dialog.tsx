import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { EntityTagsField } from '@/routes/identity/-entity-tags'
import { GeneratedUrlInput, OnboardingResultSummary } from '@/routes/identity/-onboarding-result'
import { useCreateUser, type CreateUserState } from '@/routes/identity/-users/use-create-user'
import type { IdentityTransition } from '@/routes/identity/-use-identity-transition'
import {
  AuthMethodSelect,
  MembershipFields,
  SsoProviderField,
} from '@/routes/identity/-user-form-fields'
import { isSsoProviderMissing, type UserFormOptions } from '@/routes/identity/-user-form'
import type { CreateUserResult } from '@/types/api'

/**
 * The `Add user` dialog. Its form stays filled when the dialog is dismissed, and Cancel or Close
 * clears it.
 */
export function CreateUserDialog({
  open,
  onOpenChange,
  teams,
  oidcProviders,
  oauthProviders,
  transition,
}: UserFormOptions & {
  open: boolean
  onOpenChange: (open: boolean) => void
  transition: IdentityTransition
}) {
  const createUser = useCreateUser({ oidcProviders, oauthProviders, transition })

  function close() {
    createUser.reset()
    onOpenChange(false)
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        onOpenChange(nextOpen)
        if (!nextOpen) {
          createUser.clearResult()
        }
      }}
    >
      <DialogTrigger asChild>
        <Button type="button">Add user</Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add user</DialogTitle>
          <DialogDescription>
            Pre-provision the account and generate the onboarding URL to share.
          </DialogDescription>
        </DialogHeader>

        {createUser.result ? (
          <CreatedUserResult result={createUser.result} onClose={close} />
        ) : (
          <CreateUserForm
            createUser={createUser}
            teams={teams}
            oidcProviders={oidcProviders}
            oauthProviders={oauthProviders}
            isPending={transition.isPending}
            onCancel={close}
          />
        )}
      </DialogContent>
    </Dialog>
  )
}

function CreatedUserResult({ result, onClose }: { result: CreateUserResult; onClose: () => void }) {
  return (
    <div className="flex flex-col gap-4">
      <Alert>
        <OnboardingResultSummary result={result} />
      </Alert>

      <FieldGroup>
        <Field>
          <FieldLabel htmlFor="generated-url">Generated URL</FieldLabel>
          <GeneratedUrlInput id="generated-url" result={result} />
        </Field>
      </FieldGroup>

      <DialogFooter>
        <Button type="button" variant="secondary" onClick={onClose}>
          Close
        </Button>
      </DialogFooter>
    </div>
  )
}

function CreateUserForm({
  createUser,
  teams,
  oidcProviders,
  oauthProviders,
  isPending,
  onCancel,
}: UserFormOptions & {
  createUser: CreateUserState
  isPending: boolean
  onCancel: () => void
}) {
  const { form, setForm, setAuthMode, submit } = createUser

  return (
    <form className="flex flex-col gap-6" onSubmit={submit}>
      <FieldGroup>
        <Field>
          <FieldLabel htmlFor="name">Name</FieldLabel>
          <Input
            id="name"
            value={form.name}
            onChange={(event) => setForm((current) => ({ ...current, name: event.target.value }))}
            placeholder="Jane Operator"
            required
          />
        </Field>

        <Field>
          <FieldLabel htmlFor="email">Email</FieldLabel>
          <Input
            id="email"
            type="email"
            value={form.email}
            onChange={(event) => setForm((current) => ({ ...current, email: event.target.value }))}
            placeholder="jane@example.com"
            required
          />
          <FieldDescription>
            Budget threshold alerts use this email in the initial rollout, so it must be valid and
            monitored.
          </FieldDescription>
        </Field>

        <Field>
          <FieldLabel htmlFor="auth-mode">Auth method</FieldLabel>
          <AuthMethodSelect id="auth-mode" value={form.auth_mode} onValueChange={setAuthMode} />
        </Field>

        {form.auth_mode === 'oidc' ? (
          <SsoProviderField
            id="oidc-provider"
            label="OIDC provider"
            providers={oidcProviders}
            value={form.oidc_provider_key}
            onValueChange={(value) =>
              setForm((current) => ({ ...current, oidc_provider_key: value }))
            }
            missingTitle="No SSO providers configured"
            missingDescription="Add an OIDC provider in the gateway before inviting users with SSO, or use password onboarding for now."
            description="Activation happens after a successful redirect back from this provider."
          />
        ) : null}

        {form.auth_mode === 'oauth' ? (
          <SsoProviderField
            id="oauth-provider"
            label="OAuth provider"
            providers={oauthProviders}
            value={form.oauth_provider_key}
            onValueChange={(value) =>
              setForm((current) => ({ ...current, oauth_provider_key: value }))
            }
            missingTitle="No OAuth providers configured"
            missingDescription="Add an OAuth provider in the gateway before inviting users with SSO, or use password onboarding for now."
            description="Activation happens after a successful redirect back from this provider."
          />
        ) : null}

        <MembershipFields idPrefix="" form={form} setForm={setForm} teams={teams} />

        <EntityTagsField
          label="Tags"
          tags={form.tags}
          onChange={(tags) => setForm((current) => ({ ...current, tags }))}
        />
      </FieldGroup>

      <DialogFooter>
        <Button type="button" variant="secondary" onClick={onCancel}>
          Cancel
        </Button>
        <Button
          type="submit"
          disabled={isPending || isSsoProviderMissing(form, oidcProviders, oauthProviders)}
        >
          {isPending ? 'Creating…' : 'Create user'}
        </Button>
      </DialogFooter>
    </form>
  )
}
