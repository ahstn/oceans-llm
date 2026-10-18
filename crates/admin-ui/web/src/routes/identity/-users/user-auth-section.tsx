import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { GeneratedUrlInput, OnboardingResultSummary } from '@/routes/identity/-onboarding-result'
import type { OnboardingReset } from '@/routes/identity/-users/use-onboarding-reset'
import type { UserDetails } from '@/routes/identity/-users/use-user-details'
import { isSsoProviderMissing } from '@/routes/identity/-user-form'
import { AuthMethodSelect, SsoProviderField } from '@/routes/identity/-user-form-fields'
import type { IdentityUsersPayload, UserView } from '@/types/api'

interface LifecycleProps {
  user: UserView
  details: UserDetails
  onboarding: OnboardingReset
  isPending: boolean
}

/** Auth mode and provider, editable until the user activates, plus the lifecycle actions. */
export function UserAuthSection({
  user,
  details,
  onboarding,
  oidcProviders,
  oauthProviders,
  isPending,
}: LifecycleProps & {
  oidcProviders: IdentityUsersPayload['oidc_providers']
  oauthProviders: IdentityUsersPayload['oauth_providers']
}) {
  const { updateForm, setUpdateForm } = details
  const isInvited = user.status === 'invited'

  return (
    <div className="flex flex-col gap-4">
      <Alert>
        <AlertTitle>
          {isInvited ? 'Auth mode can still be changed' : 'Auth mode is locked after activation'}
        </AlertTitle>
        <AlertDescription>
          {isInvited
            ? 'You can switch onboarding mode before the user completes setup.'
            : 'Use reset onboarding to reissue credentials for invited or disabled users.'}
        </AlertDescription>
      </Alert>

      <FieldGroup>
        <Field>
          <FieldLabel htmlFor="manage-auth-mode">Auth method</FieldLabel>
          <AuthMethodSelect
            id="manage-auth-mode"
            value={updateForm.auth_mode}
            onValueChange={details.setUpdateAuthMode}
            disabled={!isInvited}
          />
          <FieldDescription>
            {isInvited
              ? 'You can switch onboarding mode before the user completes setup.'
              : 'Auth mode is read-only after activation.'}
          </FieldDescription>
        </Field>

        {updateForm.auth_mode === 'oidc' ? (
          <SsoProviderField
            id="manage-oidc-provider"
            label="OIDC provider"
            providers={oidcProviders}
            value={updateForm.oidc_provider_key}
            onValueChange={(value) =>
              setUpdateForm((current) => ({ ...current, oidc_provider_key: value }))
            }
            disabled={!isInvited}
            missingTitle="No SSO providers configured"
            missingDescription="Add an OIDC provider in the gateway before switching a user to SSO."
          />
        ) : null}

        {updateForm.auth_mode === 'oauth' ? (
          <SsoProviderField
            id="manage-oauth-provider"
            label="OAuth provider"
            providers={oauthProviders}
            value={updateForm.oauth_provider_key}
            onValueChange={(value) =>
              setUpdateForm((current) => ({ ...current, oauth_provider_key: value }))
            }
            disabled={!isInvited}
            missingTitle="No OAuth providers configured"
            missingDescription="Add an OAuth provider in the gateway before switching a user to SSO."
          />
        ) : null}
      </FieldGroup>

      <UserLifecycleActions
        user={user}
        details={details}
        onboarding={onboarding}
        isPending={isPending}
        // An invited user's auth change is saved before the reset, so it needs a provider.
        resetBlocked={isInvited && isSsoProviderMissing(updateForm, oidcProviders, oauthProviders)}
      />
    </div>
  )
}

function UserLifecycleActions({
  user,
  details,
  onboarding,
  isPending,
  resetBlocked,
}: LifecycleProps & {
  /** Reset would first save an auth change that has no provider. */
  resetBlocked: boolean
}) {
  const { onboardingResult } = onboarding

  return (
    <section className="border-border flex flex-col gap-3 rounded-lg border p-4">
      <div className="flex flex-col gap-1">
        <h3 className="text-foreground text-sm font-semibold">Lifecycle actions</h3>
        <p className="text-subtle-foreground text-sm">
          These operations take effect immediately and the list will refresh from the gateway after
          each action.
        </p>
      </div>

      <div className="flex flex-wrap gap-2">
        {user.status !== 'disabled' ? (
          <Button
            type="button"
            variant="destructive"
            onClick={details.deactivateUser}
            disabled={isPending}
          >
            Deactivate
          </Button>
        ) : (
          <Button
            type="button"
            variant="secondary"
            onClick={details.reactivateUser}
            disabled={isPending}
          >
            Reactivate
          </Button>
        )}

        <Button
          type="button"
          variant="ghost"
          onClick={onboarding.resetOnboarding}
          disabled={
            isPending || (user.status !== 'invited' && user.status !== 'disabled') || resetBlocked
          }
        >
          Reset onboarding
        </Button>
      </div>

      {onboardingResult ? (
        <Alert>
          <OnboardingResultSummary result={onboardingResult} />

          <Field className="mt-3">
            <FieldLabel htmlFor="reset-onboarding-url">Generated URL</FieldLabel>
            <GeneratedUrlInput id="reset-onboarding-url" result={onboardingResult} />
          </Field>
        </Alert>
      ) : null}
    </section>
  )
}
