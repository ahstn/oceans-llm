import { expect, test } from 'playwright/test'

import { requireEnv } from './env'

function extractAttribute(tag: string, name: string): string | null {
  const match = tag.match(new RegExp(`\\s${name}=(['"])(.*?)\\1`, 'i'))
  return match?.[2] ?? null
}

function assetUrlsFromHtml(
  html: string,
  root: string,
  selector: 'stylesheet' | 'script',
): Array<URL> {
  const tags =
    selector === 'stylesheet'
      ? Array.from(html.matchAll(/<link\b[^>]*>/gi), (match) => match[0]).filter((tag) =>
          extractAttribute(tag, 'rel')
            ?.toLowerCase()
            .split(/\s+/)
            .includes('stylesheet'),
        )
      : Array.from(html.matchAll(/<script\b[^>]*>/gi), (match) => match[0])

  return tags
    .map((tag) => extractAttribute(tag, selector === 'stylesheet' ? 'href' : 'src'))
    .filter((asset): asset is string => Boolean(asset))
    .map((asset) => new URL(asset, root))
}

test('admin shell serves linked CSS and JavaScript assets through the gateway', async ({
  request,
  baseURL,
}) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  const shellResponse = await request.get(`${root}/admin`)

  expect(shellResponse.status()).toBe(200)
  expect(shellResponse.headers()['content-type']).toMatch(/^text\/html\b/i)

  const shellHtml = await shellResponse.text()
  const stylesheetUrls = assetUrlsFromHtml(shellHtml, root, 'stylesheet')
  const scriptUrls = assetUrlsFromHtml(shellHtml, root, 'script')

  const stylesheetUrl = stylesheetUrls.find((url) =>
    /^\/admin\/assets\/.*\.css$/.test(url.pathname),
  )
  const scriptUrl = scriptUrls.find((url) => /^\/admin\/assets\/.*\.js$/.test(url.pathname))

  if (!stylesheetUrl || !scriptUrl) {
    throw new Error('expected the admin shell to link built CSS and JavaScript assets')
  }

  const stylesheetResponse = await request.get(stylesheetUrl.href)
  expect(stylesheetResponse.status()).toBe(200)
  expect(stylesheetResponse.headers()['content-type']).toMatch(/^text\/css\b/i)

  const stylesheetBody = await stylesheetResponse.text()
  expect(stylesheetBody).toMatch(/\{[^}]+\}/)
  expect(stylesheetBody).not.toMatch(/<!doctype html|<html\b|Admin sign in/i)

  const scriptResponse = await request.get(scriptUrl.href)
  expect(scriptResponse.status()).toBe(200)
  expect(scriptResponse.headers()['content-type']).toMatch(
    /^(?:application|text)\/javascript\b/i,
  )

  const scriptBody = await scriptResponse.text()
  expect(scriptBody.trimStart()).not.toMatch(/^(?:<!doctype html|<html\b)/i)
})

test('unauthenticated protected admin routes still require login', async ({ page }) => {
  await page.goto('/admin/api-keys')

  await expect(page).toHaveURL(/\/admin\/login\?redirect=%2Fapi-keys$/)
  await expect(page.getByText('Admin sign in')).toBeVisible()
})
