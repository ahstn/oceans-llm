import { type APIRequestContext, expect, type Page } from 'playwright/test'

const adminEmail = process.env.E2E_ADMIN_EMAIL ?? 'admin@local'
const adminPassword = process.env.E2E_ADMIN_PASSWORD ?? 'admin'
const newPassword = process.env.E2E_ADMIN_NEW_PASSWORD ?? 's3cur3-passw0rd'

function parseCookieHeader(setCookie: string): { name: string; value: string } {
  const [pair] = setCookie.split(';')
  const [name, ...valueParts] = pair.split('=')
  return {
    name: name.trim(),
    value: valueParts.join('=').trim(),
  }
}

async function login(
  request: APIRequestContext,
  root: string,
  password: string,
): Promise<{
  response: Awaited<ReturnType<APIRequestContext['post']>>
  cookieHeader: string | null
  mustChangePassword: boolean
}> {
  const response = await request.post(`${root}/api/v1/auth/login/password`, {
    headers: {
      'content-type': 'application/json',
    },
    data: {
      email: adminEmail,
      password,
    },
  })

  if (response.status() !== 200) {
    return {
      response,
      cookieHeader: null,
      mustChangePassword: false,
    }
  }

  const body = (await response.json()) as {
    data: {
      must_change_password: boolean
    }
  }
  const setCookie = response.headers()['set-cookie']
  if (!setCookie) {
    throw new Error('login response did not include a set-cookie header')
  }
  const parsed = parseCookieHeader(setCookie)
  return {
    response,
    cookieHeader: `${parsed.name}=${parsed.value}`,
    mustChangePassword: body.data.must_change_password,
  }
}

export async function ensureAdminSession(
  page: Page,
  request: APIRequestContext,
  root: string,
): Promise<string> {
  for (const candidate of [newPassword, adminPassword]) {
    const result = await login(request, root, candidate)
    if (result.response.status() !== 200 || !result.cookieHeader) {
      continue
    }

    let sessionCookie = result.cookieHeader
    if (result.mustChangePassword) {
      const rotateResponse = await request.post(`${root}/api/v1/auth/password/change`, {
        headers: {
          'content-type': 'application/json',
          cookie: result.cookieHeader,
        },
        data: {
          current_password: candidate,
          new_password: newPassword,
        },
      })
      expect(rotateResponse.ok()).toBe(true)

      const relogin = await login(request, root, newPassword)
      expect(relogin.response.status()).toBe(200)
      if (!relogin.cookieHeader) {
        throw new Error('relogin after password change did not include a set-cookie header')
      }
      sessionCookie = relogin.cookieHeader
    }

    const separator = sessionCookie.indexOf('=')
    const name = sessionCookie.slice(0, separator)
    const value = sessionCookie.slice(separator + 1)
    await page.context().addCookies([
      {
        name,
        value,
        url: root,
      },
    ])
    return sessionCookie
  }

  throw new Error('unable to establish an admin session with configured credentials')
}
