import { useState } from 'react'
import { toast } from 'sonner'

import {
  removeIdentityUserProviderCredential,
  saveIdentityUserProviderCredential,
} from '@/server/admin-data.functions'
import type { IdentityTransition } from '@/routes/identity/-use-identity-transition'

const emptyProviderTokens: Record<string, string> = {}

export type ProviderCredentials = ReturnType<typeof useProviderCredentials>

/**
 * Unsaved provider token drafts, kept per user, and the actions that store or remove a token.
 * A finished request clears only the draft it sent, so newer typing and other users' drafts stay.
 */
export function useProviderCredentials(userId: string | null, transition: IdentityTransition) {
  const [tokensByUser, setTokensByUser] = useState<Record<string, Record<string, string>>>({})
  const tokens = userId ? (tokensByUser[userId] ?? emptyProviderTokens) : emptyProviderTokens

  function updateToken(draftUserId: string, providerKey: string, token: string) {
    setTokensByUser((current) => ({
      ...current,
      [draftUserId]: { ...current[draftUserId], [providerKey]: token },
    }))
  }

  function clearTokenIfUnchanged(draftUserId: string, providerKey: string, expectedToken: string) {
    setTokensByUser((current) => {
      const userTokens = current[draftUserId]
      if ((userTokens?.[providerKey] ?? '') !== expectedToken) {
        return current
      }
      return { ...current, [draftUserId]: { ...userTokens, [providerKey]: '' } }
    })
  }

  function save(providerKey: string) {
    if (!userId) return
    const tokenDraft = tokens[providerKey] ?? ''
    const token = tokenDraft.trim()
    if (!token) {
      toast.error('Enter a GitHub token')
      return
    }
    transition.run(async () => {
      await saveIdentityUserProviderCredential({
        data: { userId, providerKey, token },
      })
      clearTokenIfUnchanged(userId, providerKey, tokenDraft)
      toast.success('Provider token saved')
      await transition.refresh()
    })
  }

  function remove(providerKey: string) {
    if (!userId) return
    const tokenDraft = tokens[providerKey] ?? ''
    transition.run(async () => {
      await removeIdentityUserProviderCredential({
        data: { userId, providerKey },
      })
      clearTokenIfUnchanged(userId, providerKey, tokenDraft)
      toast.success('Provider token removed')
      await transition.refresh()
    })
  }

  function clearAll() {
    setTokensByUser({})
  }

  return { tokens, updateToken, save, remove, clearAll }
}
