import type { ApiKeysPayload, ApiKeyView, MyProfileDayView, MyProfileView } from '@/types/api'

export function profileDay(
  day: string,
  overrides: Partial<MyProfileDayView> = {},
): MyProfileDayView {
  return {
    day,
    request_count: 4,
    input_tokens: 1_000,
    output_tokens: 200,
    uncached_input_tokens: 600,
    cache_read_tokens: 400,
    cache_write_tokens: 0,
    total_tokens: 1_200,
    cost_usd_10000: 25_000,
    ...overrides,
  }
}

/** Window ends at the start of 2026-09-26, so the last profile day is Friday 2026-09-25. */
export function profileView(overrides: Partial<MyProfileView> = {}): MyProfileView {
  return {
    window_start: '2025-09-26T00:00:00Z',
    window_end: '2026-09-26T00:00:00Z',
    budget: {
      settings: {
        amount_usd: '100.00',
        amount_usd_10000: 1_000_000,
        cadence: 'weekly',
        hard_limit: false,
        timezone: 'UTC',
      },
      source: { kind: 'manual', key: null },
      period_start: '2026-09-21T00:00:00Z',
      period_end: '2026-09-28T00:00:00Z',
      spent_usd_10000: 250_000,
    },
    days: [
      profileDay('2026-09-23', {
        request_count: 2,
        total_tokens: 500,
        input_tokens: 400,
        output_tokens: 100,
      }),
      profileDay('2026-09-24'),
      profileDay('2026-09-25', {
        request_count: 10,
        total_tokens: 9_000,
        input_tokens: 8_000,
        output_tokens: 1_000,
      }),
    ],
    model_days: [
      { day: '2026-09-23', model_key: 'fast', request_count: 2, total_tokens: 500 },
      { day: '2026-09-24', model_key: 'fast', request_count: 1, total_tokens: 300 },
      { day: '2026-09-24', model_key: 'reasoning', request_count: 3, total_tokens: 900 },
      { day: '2026-09-25', model_key: 'reasoning', request_count: 10, total_tokens: 9_000 },
    ],
    harness_days: [
      {
        day: '2026-09-24',
        harness_key: 'codex',
        harness_label: 'Codex',
        request_count: 4,
        total_tokens: 1_200,
      },
      {
        day: '2026-09-25',
        harness_key: 'claude_code',
        harness_label: 'Claude Code',
        request_count: 10,
        total_tokens: 9_000,
      },
    ],
    ...overrides,
  }
}

export function apiKey(id: string, overrides: Partial<ApiKeyView> = {}): ApiKeyView {
  return {
    id,
    name: `Key ${id}`,
    prefix: `gwk_${id}_abcdefghijkl`,
    status: 'active',
    owner_kind: 'user',
    owner_id: 'user_1',
    owner_name: 'Jane User',
    owner_email: 'jane@example.com',
    owner_team_key: null,
    owner_service_account_key: null,
    owner_service_account_team_id: null,
    owner_service_account_team_key: null,
    model_grant_mode: 'all',
    model_keys: [],
    created_at: '2026-09-01T00:00:00Z',
    last_used_at: null,
    revoked_at: null,
    ...overrides,
  }
}

export function apiKeysPayload(items: ApiKeyView[]): ApiKeysPayload {
  return { items, users: [], service_accounts: [], models: [] }
}
