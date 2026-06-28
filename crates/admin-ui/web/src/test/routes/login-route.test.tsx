import { describe, expect, it, vi } from 'vitest'

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => ({}),
}))

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}))

vi.mock('@/server/admin-data.functions', () => ({
  getOidcLoginOptions: vi.fn(),
  loginAdminWithPassword: vi.fn(),
}))

describe('login SSO errors', () => {
  it('explains GitHub unverified primary email failures', async () => {
    const { ssoErrorMessage } = await import('@/routes/login')

    expect(ssoErrorMessage('github_unverified_email')).toContain(
      'https://github.com/settings/emails',
    )
  })
})
