import { canPerformAdminAction } from '@/routes/-auth-routing'
import { getMyProfile } from '@/server/admin-data.functions'
import type { AdminAction, ApiKeyView, AuthSessionView, MyProfileView } from '@/types/api'

import type { ApiKeyLinks } from './profile-api-keys'
import { personalApiKeys } from './profile-data'

export type ProfileLoaderData = {
  profile: MyProfileView
}

/** Loads the profile, which carries the viewer's own API keys alongside their usage. */
export async function loadProfilePage(): Promise<ProfileLoaderData> {
  const profile = await getMyProfile()
  return { profile: profile.data }
}

export type ProfilePageModel = {
  profile: MyProfileView
  keys: ApiKeyView[]
  links: ApiKeyLinks
}

export function profilePageModel(
  data: ProfileLoaderData,
  session: AuthSessionView,
): ProfilePageModel {
  const hasKeysPage = session.permissions.pages.includes('api_keys')
  const can = (action: AdminAction) => hasKeysPage && canPerformAdminAction(session, action)
  return {
    profile: data.profile,
    keys: personalApiKeys(data.profile.api_keys, session.user.id),
    links: {
      canView: hasKeysPage,
      // Matches the API keys route, which only opens a linked key for these actions.
      canManage: can('update_api_key') || can('revoke_api_key') || can('reveal_api_key'),
      canCreate: can('create_api_key'),
    },
  }
}
