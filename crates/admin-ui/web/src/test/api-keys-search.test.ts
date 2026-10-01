import { defaultParseSearch, defaultStringifySearch } from '@tanstack/react-router'
import { describe, expect, it } from 'vitest'

import { validateApiKeysSearch } from '@/routes/api-keys/-use-api-keys-page'

describe('validateApiKeysSearch', () => {
  it('opens the create dialog from the URL the profile page links to', () => {
    const url = defaultStringifySearch({ create: true })
    expect(url).toBe('?create=true')
    expect(validateApiKeysSearch(defaultParseSearch(url))).toEqual({
      api_key_id: undefined,
      create: true,
    })
  })

  it.each(['true', '1', 1])('accepts create=%s', (create) => {
    expect(validateApiKeysSearch({ create }).create).toBe(true)
  })

  it.each([false, 'false', '0', undefined])('ignores create=%s', (create) => {
    expect(validateApiKeysSearch({ create }).create).toBeUndefined()
  })

  it('keeps a string api_key_id only', () => {
    expect(validateApiKeysSearch({ api_key_id: 'key-1' }).api_key_id).toBe('key-1')
    expect(validateApiKeysSearch({ api_key_id: 42 }).api_key_id).toBeUndefined()
  })
})
