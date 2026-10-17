import { useMemo } from 'react'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { GeneratedUrlInput } from '@/routes/identity/-onboarding-result'
import { existingMemberOptionsFor } from '@/routes/identity/-team-form'
import type { AddMembers } from '@/routes/identity/-use-add-members'
import { AuthMethodSelect, SsoProviderField } from '@/routes/identity/-user-form-fields'
import { isSsoProviderMissing } from '@/routes/identity/-user-form'
import { UserMultiSelectField } from '@/routes/identity/-user-multi-select-field'
import type {
  CreateUserResult,
  IdentityTeamsPayload,
  TeamAssignableUserView,
  TeamManagementView,
} from '@/types/api'

interface SsoProviders {
  oidcProviders: IdentityTeamsPayload['oidc_providers']
  oauthProviders: IdentityTeamsPayload['oauth_providers']
}

/** The `Add members` dialog: add existing users to the team, or invite a new member into it. */
export function AddMembersDialog({
  addMembers,
  users,
  oidcProviders,
  oauthProviders,
  isPending,
}: SsoProviders & {
  addMembers: AddMembers
  users: TeamAssignableUserView[]
  isPending: boolean
}) {
  const { team } = addMembers

  return (
    <Dialog open={addMembers.isOpen} onOpenChange={(open) => !open && addMembers.close()}>
      <DialogContent className="w-[min(760px,calc(100vw-32px))]">
        <DialogHeader>
          <DialogTitle>Add members</DialogTitle>
          <DialogDescription>
            {team
              ? `Add existing users to ${team.name} or invite a new member directly into the team.`
              : 'Add existing users or invite a new member.'}
          </DialogDescription>
        </DialogHeader>

        {team ? (
          <div className="flex flex-col gap-6">
            <ExistingUsersSection
              team={team}
              users={users}
              addMembers={addMembers}
              isPending={isPending}
            />

            <InviteMemberSection
              team={team}
              addMembers={addMembers}
              oidcProviders={oidcProviders}
              oauthProviders={oauthProviders}
              isPending={isPending}
            />

            <DialogFooter>
              <Button type="button" variant="secondary" onClick={addMembers.close}>
                Close
              </Button>
            </DialogFooter>
          </div>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

function ExistingUsersSection({
  team,
  users,
  addMembers,
  isPending,
}: {
  team: TeamManagementView
  users: TeamAssignableUserView[]
  addMembers: AddMembers
  isPending: boolean
}) {
  const options = useMemo(() => existingMemberOptionsFor(users, team), [users, team])

  return (
    <section className="border-border flex flex-col gap-4 rounded-lg border p-5">
      <div className="flex flex-col gap-1">
        <h3 className="text-foreground text-base font-semibold">Existing users</h3>
        <p className="text-subtle-foreground text-sm">
          Only teamless users can be newly added. Users already on another team remain unavailable
          in this flow.
        </p>
      </div>

      <form className="flex flex-col gap-4" onSubmit={addMembers.addSelectedUsers}>
        <UserMultiSelectField
          label="Users"
          description="Select existing users to add as members."
          placeholder="Select existing users"
          users={options}
          selectedUserIds={addMembers.selectedUserIds}
          onChange={addMembers.setSelectedUserIds}
          emptyTitle="No existing users available"
          emptyDescription="Create or invite a new user below if the team is being set up first."
        />

        <div className="flex justify-end">
          <Button
            type="submit"
            variant="secondary"
            disabled={isPending || addMembers.selectedUserIds.length === 0}
          >
            {isPending ? 'Adding…' : 'Add selected users'}
          </Button>
        </div>
      </form>
    </section>
  )
}

/** Invites a new user straight into the team as a member, then shows the onboarding URL. */
function InviteMemberSection({
  team,
  addMembers,
  oidcProviders,
  oauthProviders,
  isPending,
}: SsoProviders & {
  team: TeamManagementView
  addMembers: AddMembers
  isPending: boolean
}) {
  return (
    <section className="border-border flex flex-col gap-4 rounded-lg border p-5">
      <div className="flex flex-col gap-1">
        <h3 className="text-foreground text-base font-semibold">Invite a new member</h3>
        <p className="text-subtle-foreground text-sm">
          This uses the same onboarding flow as the users page and preassigns the new user to{' '}
          {team.name} as a member.
        </p>
      </div>

      <div className="flex flex-col gap-4">
        {addMembers.inviteResult ? (
          <Alert>
            <MemberInviteSummary result={addMembers.inviteResult} />
          </Alert>
        ) : null}

        <InviteMemberForm
          addMembers={addMembers}
          oidcProviders={oidcProviders}
          oauthProviders={oauthProviders}
          isPending={isPending}
        />
      </div>
    </section>
  )
}

/** Name, email, and auth method. The generated URL appears in the form once the invite exists. */
function InviteMemberForm({
  addMembers,
  oidcProviders,
  oauthProviders,
  isPending,
}: SsoProviders & {
  addMembers: AddMembers
  isPending: boolean
}) {
  const { inviteForm, setInviteForm, inviteResult } = addMembers

  return (
    <form className="flex flex-col gap-5" onSubmit={addMembers.inviteMember}>
      <FieldGroup>
        <Field>
          <FieldLabel htmlFor="member-name">Name</FieldLabel>
          <Input
            id="member-name"
            value={inviteForm.name}
            onChange={(event) =>
              setInviteForm((current) => ({ ...current, name: event.target.value }))
            }
            placeholder="Taylor Member"
            required
          />
        </Field>

        <Field>
          <FieldLabel htmlFor="member-email">Email</FieldLabel>
          <Input
            id="member-email"
            type="email"
            value={inviteForm.email}
            onChange={(event) =>
              setInviteForm((current) => ({ ...current, email: event.target.value }))
            }
            placeholder="taylor@example.com"
            required
          />
        </Field>

        <Field>
          <FieldLabel htmlFor="member-auth-mode">Auth method</FieldLabel>
          <AuthMethodSelect
            id="member-auth-mode"
            value={inviteForm.auth_mode}
            onValueChange={addMembers.setInviteAuthMode}
          />
        </Field>

        {inviteForm.auth_mode === 'oidc' ? (
          <SsoProviderField
            id="member-oidc-provider"
            label="OIDC provider"
            providers={oidcProviders}
            value={inviteForm.oidc_provider_key}
            onValueChange={(value) =>
              setInviteForm((current) => ({ ...current, oidc_provider_key: value }))
            }
            missingTitle="No SSO providers configured"
            missingDescription="Add an OIDC provider in the gateway before inviting members with SSO, or use password onboarding for now."
            description="Activation happens after a successful redirect back from this provider."
          />
        ) : null}

        {inviteForm.auth_mode === 'oauth' ? (
          <SsoProviderField
            id="member-oauth-provider"
            label="OAuth provider"
            providers={oauthProviders}
            value={inviteForm.oauth_provider_key}
            onValueChange={(value) =>
              setInviteForm((current) => ({ ...current, oauth_provider_key: value }))
            }
            missingTitle="No OAuth providers configured"
            missingDescription="Add an OAuth provider in the gateway before inviting members with SSO, or use password onboarding for now."
            description="Activation happens after a successful redirect back from this provider."
          />
        ) : null}

        {inviteResult ? (
          <Field>
            <FieldLabel htmlFor="generated-member-url">Generated URL</FieldLabel>
            <GeneratedUrlInput id="generated-member-url" result={inviteResult} />
          </Field>
        ) : null}
      </FieldGroup>

      <div className="flex justify-end">
        <Button
          type="submit"
          disabled={isPending || isSsoProviderMissing(inviteForm, oidcProviders, oauthProviders)}
        >
          {isPending ? 'Creating…' : 'Invite member'}
        </Button>
      </div>
    </form>
  )
}

/** Worded for a team member, unlike the users page's `Password invite ready`. */
function MemberInviteSummary({ result }: { result: CreateUserResult }) {
  return (
    <>
      <AlertTitle>
        {result.kind === 'password_invite' ? 'Member invite ready' : 'Member sign-in URL ready'}
      </AlertTitle>
      <AlertDescription>
        {result.kind === 'password_invite'
          ? `Share this invite before ${result.expires_at}.`
          : `Share this URL with ${result.user.email} so they can finish SSO onboarding.`}
      </AlertDescription>
    </>
  )
}
