import { type APIRequestContext, expect } from 'playwright/test'

import { loginWithPasswordSession } from './admin-session'

export type ActiveRegularUser = {
  id: string
  name: string
  email: string
  cookie: string
}

export function invitationToken(inviteUrl: string, root: string): string {
  const token = new URL(inviteUrl, root).pathname.split('/').filter(Boolean).pop()
  if (!token) {
    throw new Error(`expected password invite URL to include a token: ${inviteUrl}`)
  }
  return token
}

export async function createActiveRegularUser(
  request: APIRequestContext,
  root: string,
  adminCookie: string,
  label: string,
): Promise<ActiveRegularUser> {
  const unique = `${Date.now()}-${Math.random().toString(36).slice(2)}`
  const name = `${label} User`
  const email = `${label}-${unique}@example.com`
  const password = `${label}-passw0rd`
  const createResponse = await request.post(`${root}/api/v1/admin/identity/users`, {
    headers: {
      cookie: adminCookie,
      'content-type': 'application/json',
    },
    data: {
      name,
      email,
      auth_mode: 'password',
      global_role: 'user',
      tags: [{ key: 'department', value: 'security' }],
    },
  })
  expect(createResponse.status()).toBe(200)
  const createBody = (await createResponse.json()) as {
    data: { kind: 'password_invite'; user: { id: string }; invite_url: string } | { kind: string }
  }
  expect(createBody.data.kind).toBe('password_invite')
  if (createBody.data.kind !== 'password_invite') {
    throw new Error(`expected password invite onboarding, received ${createBody.data.kind}`)
  }

  const completeResponse = await request.post(
    `${root}/api/v1/auth/invitations/${invitationToken(createBody.data.invite_url, root)}/password`,
    {
      headers: { 'content-type': 'application/json' },
      data: { password },
    },
  )
  expect(completeResponse.status()).toBe(200)

  return {
    id: createBody.data.user.id,
    name,
    email,
    cookie: await loginWithPasswordSession(request, root, email, password),
  }
}
