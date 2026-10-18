// Seeds a showcase Agent Skills catalog into a local Skills-enabled stack and saves the
// generated client config for the hero model. Local stack only; throwaway users live in the
// run-local database that `control-oceans-admin cleanup` removes. Passwords stay in memory.
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { randomUUID } from 'node:crypto'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const repoRoot = path.resolve(here, '../../..')
const requireFromAdminUi = createRequire(path.join(repoRoot, 'crates/admin-ui/web/package.json'))
const { chromium, request } = requireFromAdminUi('playwright')
const baseURL = process.env.OCEANS_CAPTURE_BASE_URL ?? 'http://127.0.0.1:38390'
if (!/^http:\/\/127\.0\.0\.1:\d+$/.test(baseURL)) throw new Error('local stack only')

const md = (name, description, body, extra = '') =>
  `---\nname: ${name}\ndescription: ${JSON.stringify(description)}\n${extra}---\n${body}\n`

// Oldest first: the catalog lists skills by most recent update.
const OWNERS = [
  {
    handle: 'ux', user: ['Nora Patel', 'nora.patel@example.com'],
    skill: 'frontend-polish',
    description: 'Polish a working UI: spacing rhythm, type scale, focus states, and motion that respects reduced motion.',
    versions: 4,
  },
  {
    handle: 'data', user: ['Dana Kim', 'dana.kim@example.com'],
    skill: 'warehouse-sql-review',
    description: 'Review warehouse SQL for cost and safety. Flags full scans, missing filters, and destructive statements.',
    versions: 1,
  },
  {
    handle: 'alice', user: ['Alice Moreno', 'alice.moreno@example.com'],
    skill: 'code-review',
    description: 'Review a pull request like a senior engineer: correctness first, then tests, naming, and the smallest safe change.',
    versions: 2,
  },
  {
    handle: 'security', user: ['Marcus Lee', 'marcus.lee@example.com'],
    skill: 'secret-sweep',
    description: 'Sweep a diff for leaked credentials before it leaves the laptop, then suggest the redaction and key rotation.',
    versions: 2,
  },
  {
    handle: 'platform', user: ['Priya Shah', 'priya.shah@example.com'],
    skill: 'incident-triage',
    description: 'Triage a production incident: pull gateway request logs, correlate trace IDs, and draft the status update.',
    versions: 3,
  },
]

const work = await fs.mkdtemp(path.join(os.tmpdir(), 'showreel-skills-'))
async function zipSkill(name, files) {
  const dir = path.join(work, `${name}-${randomUUID().slice(0, 8)}`)
  await fs.mkdir(path.join(dir, name, 'references'), { recursive: true })
  for (const [file, content] of Object.entries(files)) await fs.writeFile(path.join(dir, name, file), content)
  execFileSync('/usr/bin/zip', ['-qrX', 'skill.zip', name], { cwd: dir })
  return fs.readFile(path.join(dir, 'skill.zip'))
}

async function ok(response, what) {
  if (!response.ok()) throw new Error(`${what}: HTTP ${response.status()} ${(await response.text()).slice(0, 300)}`)
  return response.json()
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

async function upload(api, owner, archives) {
  const first = await ok(
    await api.post(`${baseURL}/api/v1/skills`, { headers: { 'content-type': 'application/zip' }, data: archives[0] }),
    `upload ${owner}`,
  )
  const id = first.skill?.id ?? first.detail?.skill?.id
  for (const archive of archives.slice(1)) {
    await sleep(300)
    await ok(
      await api.post(`${baseURL}/api/v1/skills/${id}/versions`, { headers: { 'content-type': 'application/zip' }, data: archive }),
      `version ${owner}`,
    )
  }
  return id
}

const browser = await chromium.launch({ headless: true })
try {
  const page = await (await browser.newContext()).newPage()
  await page.goto(`${baseURL}/admin/login`, { waitUntil: 'domcontentloaded' })
  await page.getByRole('heading', { name: 'Sign in' }).waitFor()
  await page.waitForFunction(() => {
    const button = [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Sign in')
    return button instanceof HTMLButtonElement && !button.disabled
  }, null, { timeout: 60_000 })
  await Promise.all([
    page.waitForURL((url) => !url.pathname.endsWith('/login'), { timeout: 60_000 }),
    page.getByRole('button', { name: 'Sign in' }).click(),
  ])
  const admin = page.request

  // Hero model's generated client config (the models page "Config" dialog).
  const models = await ok(await admin.get(`${baseURL}/api/v1/admin/models`), 'models')
  const list = models.data?.items ?? models.data ?? []
  console.log('models:', list.map((m) => `${m.id}[${(m.client_configurations ?? []).map((c) => c.key).join(',')}]`).join(' '))
  const hero = list.find((m) => m.id === 'claude-sonnet-4.6') ?? list.find((m) => /sonnet/.test(m.id))
  if (hero) {
    const key = hero.key ?? hero.model_key ?? hero.id
    const configs = await ok(
      await admin.post(`${baseURL}/api/v1/admin/models/client-configs`, { data: { model_keys: [key] } }),
      'client configs',
    )
    await fs.writeFile(path.join(here, 'client-configs.json'), JSON.stringify({ hero, configs }, null, 2))
    console.log(`saved client configs for ${hero.id}`)
  }

  if (process.argv.includes('--configs-only')) process.exit(0)

  // demo-admin owns the bundled, attributed grill-me skill (repo bundled-skills/).
  const existing = await ok(await admin.get(`${baseURL}/api/v1/skills?limit=50&offset=0`), 'catalog')
  const have = new Set((existing.data ?? existing).map((s) => `${s.namespace}/${s.name}`))
  const users = await ok(await admin.get(`${baseURL}/api/v1/admin/identity/users`), 'users')
  const userList = [users.data?.items, users.data?.users, users.data, users.items, users].find(Array.isArray) ?? []; if (!userList.length) console.log("users shape:", Object.keys(users.data ?? users))
  if (!have.has('demo-admin/grill-me')) {
  if (!(await ok(await admin.get(`${baseURL}/api/v1/skills/namespace`), 'admin ns')))
    await ok(await admin.post(`${baseURL}/api/v1/skills/namespace`, { data: { handle: 'demo-admin' } }), 'admin namespace')
  const grill = await fs.readFile(path.join(repoRoot, 'bundled-skills/grill-me/SKILL.md'), 'utf8')
  const license = await fs.readFile(path.join(repoRoot, 'bundled-skills/grill-me/LICENSE'), 'utf8')
  await upload(admin, 'demo-admin', [await zipSkill('grill-me', { 'SKILL.md': grill, LICENSE: license })])
  console.log('seeded demo-admin/grill-me v1')
  }

  for (const owner of OWNERS) {
    if (have.has(`${owner.handle}/${owner.skill}`)) { console.log(`kept ${owner.handle}/${owner.skill}`); continue }
    await sleep(1200)
    const [name, email] = owner.user
    const prior = userList.find((u) => (u.email ?? u.email_normalized) === email)
    const created = prior
      ? await ok(await admin.post(`${baseURL}/api/v1/admin/identity/users/${prior.id ?? prior.user_id}/password-invite`, { data: {} }), `reinvite ${owner.handle}`)
      : await ok(
          await admin.post(`${baseURL}/api/v1/admin/identity/users`, {
            data: { name, email, auth_mode: 'password', global_role: 'user', tags: [] },
          }),
          `create ${owner.handle}`,
        )
    const token = new URL(created.data.invite_url, baseURL).pathname.split('/').findLast(Boolean)
    const password = `Showreel-${randomUUID()}-9`
    const anonymous = await request.newContext()
    await ok(await anonymous.post(`${baseURL}/api/v1/auth/invitations/${token}/password`, { data: { password } }), 'invite')
    await anonymous.dispose()
    const api = await request.newContext()
    await ok(await api.post(`${baseURL}/api/v1/auth/login/password`, { data: { email, password } }), 'login')
    if (!(await ok(await api.get(`${baseURL}/api/v1/skills/namespace`), 'ns')))
      await ok(await api.post(`${baseURL}/api/v1/skills/namespace`, { data: { handle: owner.handle } }), 'namespace')
    const archives = []
    for (let v = 1; v <= owner.versions; v += 1) {
      archives.push(await zipSkill(owner.skill, {
        'SKILL.md': md(owner.skill, owner.description,
          `# ${owner.skill}\n\nFollow references/checklist.md and report findings in order of impact.`,
          `metadata:\n  author: ${name}\n`),
        'references/checklist.md': `Checklist revision ${v}.\n`,
      }))
    }
    await upload(api, owner.handle, archives)
    await api.dispose()
    console.log(`seeded ${owner.handle}/${owner.skill} v${owner.versions}`)
  }
  const catalog = await ok(await admin.get(`${baseURL}/api/v1/skills?limit=50&offset=0`), 'catalog')
  console.log('catalog:', (catalog.data ?? catalog).map((s) => `${s.namespace}/${s.name} v${s.latest_version}`).join(' | '))
} finally {
  await browser.close()
  await fs.rm(work, { recursive: true, force: true })
}
