import { canPerformAdminAction } from '@/routes/-auth-routing'
import { getMyProfile } from '@/server/admin-data.functions'
import type { ApiKeyView, AuthSessionView, MyProfileView } from '@/types/api'

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
  return {
    profile: data.profile,
    keys: personalApiKeys(data.profile.api_keys, session.user.id),
    links: {
      canManage: hasKeysPage,
      canCreate: hasKeysPage && canPerformAdminAction(session, 'create_api_key'),
    },
  }
}
