import type { Dispatch, SetStateAction } from 'react'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import type { IdentityUsersPayload, UpdateUserInput } from '@/types/api'

type Membership = Pick<UpdateUserInput, 'global_role' | 'team_id' | 'team_role'>

export function AuthMethodSelect({
  id,
  value,
  onValueChange,
  disabled,
}: {
  id: string
  value: string | undefined
  onValueChange: (authMode: string) => void
  disabled?: boolean
}) {
  return (
    <Select value={value} onValueChange={onValueChange} disabled={disabled}>
      <SelectTrigger id={id}>
        <SelectValue placeholder="Select auth method" />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          <SelectItem value="password">Password</SelectItem>
          <SelectItem value="oidc">SSO (OIDC)</SelectItem>
          <SelectItem value="oauth">SSO (OAuth)</SelectItem>
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}

/** Provider picker for an SSO auth mode, with a notice when no provider is configured. */
export function SsoProviderField({
  id,
  label,
  providers,
  value,
  onValueChange,
  disabled,
  missingTitle,
  missingDescription,
  description,
}: {
  id: string
  label: string
  providers: IdentityUsersPayload['oidc_providers'] | IdentityUsersPayload['oauth_providers']
  value: string | null | undefined
  onValueChange: (providerKey: string) => void
  disabled?: boolean
  missingTitle: string
  missingDescription: string
  description?: string
}) {
  return (
    <>
      {providers.length === 0 ? (
        <Alert>
          <AlertTitle>{missingTitle}</AlertTitle>
          <AlertDescription>{missingDescription}</AlertDescription>
        </Alert>
      ) : null}

      <Field>
        <FieldLabel htmlFor={id}>{label}</FieldLabel>
        <Select value={value ?? undefined} onValueChange={onValueChange} disabled={disabled}>
          <SelectTrigger id={id}>
            <SelectValue placeholder="Select provider" />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              {providers.map((provider) => (
                <SelectItem key={provider.key} value={provider.key}>
                  {provider.label}
                </SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>
        {description ? <FieldDescription>{description}</FieldDescription> : null}
      </Field>
    </>
  )
}

/** Global role, team, and team role. `locked` keeps an owner's team membership unchanged. */
export function MembershipFields<Form extends Membership>({
  idPrefix,
  form,
  setForm,
  teams,
  locked,
}: {
  idPrefix: '' | 'manage-'
  form: Form
  setForm: Dispatch<SetStateAction<Form>>
  teams: IdentityUsersPayload['teams']
  locked?: boolean
}) {
  return (
    <>
      <Field>
        <FieldLabel htmlFor={`${idPrefix}global-role`}>Global role</FieldLabel>
        <Select
          value={form.global_role}
          onValueChange={(value) => setForm((current) => ({ ...current, global_role: value }))}
        >
          <SelectTrigger id={`${idPrefix}global-role`}>
            <SelectValue placeholder="Select role" />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value="user">User</SelectItem>
              <SelectItem value="platform_admin">Platform admin</SelectItem>
            </SelectGroup>
          </SelectContent>
        </Select>
      </Field>

      <Field>
        <FieldLabel htmlFor={`${idPrefix}team`}>Team</FieldLabel>
        <Select
          value={form.team_id ?? 'none'}
          onValueChange={(value) =>
            setForm((current) => ({
              ...current,
              team_id: value === 'none' ? null : value,
              team_role: value === 'none' ? null : (current.team_role ?? 'member'),
            }))
          }
          disabled={locked}
        >
          <SelectTrigger id={`${idPrefix}team`}>
            <SelectValue placeholder="No team" />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value="none">No team</SelectItem>
              {teams.map((team) => (
                <SelectItem key={team.id} value={team.id}>
                  {team.name}
                </SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>
      </Field>

      {form.team_id ? (
        <Field>
          <FieldLabel htmlFor={`${idPrefix}team-role`}>Team role</FieldLabel>
          <Select
            value={form.team_role ?? 'member'}
            onValueChange={(value) => setForm((current) => ({ ...current, team_role: value }))}
            disabled={locked}
          >
            <SelectTrigger id={`${idPrefix}team-role`}>
              <SelectValue placeholder="Select team role" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value="member">Member</SelectItem>
                <SelectItem value="admin">Admin</SelectItem>
              </SelectGroup>
            </SelectContent>
          </Select>
        </Field>
      ) : null}
    </>
  )
}
