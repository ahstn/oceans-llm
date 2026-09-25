import { canPerformAdminAction } from '@/routes/-auth-routing'
import { getApiKeys, getMyProfile } from '@/server/admin-data.functions'
import type { ApiKeysPayload, ApiKeyView, AuthSessionView, MyProfileView } from '@/types/api'

import type { ApiKeyLinks } from './profile-api-keys'
import { personalApiKeys } from './profile-data'

export type ProfileLoaderData = {
  profile: MyProfileView
  keys: ApiKeysPayload
}

/** Loads the profile and the API keys shown on the page. */
export async function loadProfilePage(): Promise<ProfileLoaderData> {
  const [profile, keys] = await Promise.all([getMyProfile(), getApiKeys()])
  return { profile: profile.data, keys: keys.data }
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
    keys: personalApiKeys(data.keys.items, session.user.id),
    links: {
      canManage: hasKeysPage,
      canCreate: hasKeysPage && canPerformAdminAction(session, 'create_api_key'),
    },
  }
}
