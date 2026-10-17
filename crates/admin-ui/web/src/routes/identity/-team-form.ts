import { sanitizeEntityTags } from '@/routes/identity/-entity-tag-utils'
import { sanitizeCreateUserForm, withAuthMode } from '@/routes/identity/-user-form'
import type { UserOption } from '@/routes/identity/-user-multi-select-field'
import type {
  CreateTeamInput,
  CreateUserInput,
  IdentityTeamsPayload,
  TeamAssignableUserView,
  TeamManagementView,
  TransferTeamMemberInput,
} from '@/types/api'

type OidcProviders = IdentityTeamsPayload['oidc_providers']
type OauthProviders = IdentityTeamsPayload['oauth_providers']

export const initialTeamForm: CreateTeamInput = {
  name: '',
  admin_user_ids: [],
  tags: [],
}

export const initialInviteForm: CreateUserInput = {
  name: '',
  email: '',
  auth_mode: 'password',
  global_role: 'user',
  team_id: null,
  team_role: 'member',
  oidc_provider_key: null,
  oauth_provider_key: null,
  tags: [],
}

export const initialTransferForm: TransferTeamMemberInput = {
  destination_team_id: '',
  destination_role: 'member',
}

/** Admin candidates for the team editor. Anyone on another team is listed but can't be picked. */
export function adminOptionsFor(
  users: TeamAssignableUserView[],
  editingTeam: TeamManagementView | null,
): UserOption[] {
  return users.map((user) => {
    const onAnotherTeam = isOnAnotherTeam(user, editingTeam)
    return { user, disabled: onAnotherTeam, reason: onAnotherTeam ? alreadyInReason(user) : null }
  })
}

/**
 * Users to add to `team`. Anyone on another team is listed but can't be picked, and the team's own
 * members are marked as already on it.
 */
export function existingMemberOptionsFor(
  users: TeamAssignableUserView[],
  team: TeamManagementView,
): UserOption[] {
  return users.map((user) => {
    const onAnotherTeam = isOnAnotherTeam(user, team)
    return {
      user,
      disabled: onAnotherTeam,
      reason: onAnotherTeam
        ? alreadyInReason(user)
        : user.team_id === team.id
          ? 'Already on this team'
          : null,
    }
  })
}

/** Whether the user is on a team other than `team`, or on any team when there is no `team`. */
function isOnAnotherTeam(user: TeamAssignableUserView, team: TeamManagementView | null) {
  return user.team_id !== null && (!team || user.team_id !== team.id)
}

function alreadyInReason(user: TeamAssignableUserView) {
  return `Already in ${user.team_name ?? 'another team'}`
}

/** Whether the transfer names another team, for a member who isn't the team's owner. */
export function canTransferMember(
  form: TransferTeamMemberInput,
  team: TeamManagementView,
  user: TeamAssignableUserView,
) {
  return (
    Boolean(form.destination_team_id) &&
    form.destination_team_id !== team.id &&
    user.team_role !== 'owner'
  )
}

export function sanitizeTeamForm(form: CreateTeamInput): CreateTeamInput {
  return {
    name: form.name.trim(),
    admin_user_ids: Array.from(new Set(form.admin_user_ids)),
    tags: sanitizeEntityTags(form.tags),
  }
}

/**
 * The users page's create-user request with the invite's rules on top: it always creates a `user`
 * who joins the form's team as a member, and an SSO mode without a chosen provider uses the only
 * configured one, as switching to that mode would.
 */
export function sanitizeInviteForm(
  form: CreateUserInput,
  oidcProviders: OidcProviders,
  oauthProviders: OauthProviders,
): CreateUserInput {
  const withProvider = withAuthMode(form, form.auth_mode, oidcProviders, oauthProviders)
  return {
    ...sanitizeCreateUserForm(withProvider),
    global_role: 'user',
    team_role: 'member',
  }
}

/** A destination that is the member's current team is dropped. */
export function sanitizeTransferForm(
  form: TransferTeamMemberInput,
  team: TeamManagementView,
): TransferTeamMemberInput {
  return {
    destination_team_id:
      form.destination_team_id && form.destination_team_id !== team.id
        ? form.destination_team_id
        : '',
    destination_role: form.destination_role,
  }
}
