import { execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
import { expect, test } from 'playwright/test'

import { ensureAdminSession } from './admin-session'
import { requireEnv } from './env'
import { createActiveRegularUser } from './identity-fixtures'

test.skip(process.env.E2E_SKILLS_ENABLED !== 'true', 'Requires the real Skills S3 test profile')

const execFileAsync = promisify(execFile)
const repoRoot = fileURLToPath(new URL('../../../../', import.meta.url))

type SkillDetail = {
  skill: { id: string; namespace: string; name: string; default_version: number }
  versions: { version: number; sha256: string }[]
}

type UploadResult = SkillDetail & { uploaded_version: number }
type Preview = {
  version: {
    instructions: string
    files: { path: string }[]
    version: { version: number; sha256: string }
  }
}

async function cliExecutable(): Promise<string> {
  const executable =
    process.env.E2E_OCEANS_CLI_BIN ??
    path.join(
      process.env.CARGO_TARGET_DIR ?? path.join(repoRoot, 'target'),
      'debug',
      process.platform === 'win32' ? 'oceans.exe' : 'oceans',
    )
  try {
    await access(executable)
  } catch {
    throw new Error('Build oceans-cli first, or set E2E_OCEANS_CLI_BIN to the built oceans binary')
  }
  return executable
}

function skillInstructions(version: number): string {
  return `---\nname: cli-review\ndescription: Review code from the CLI\n---\nCLI checklist version ${version}.\n`
}

test('a user API key completes the CLI upload, edit, install, and download workflow', async ({
  page,
  request,
  baseURL,
}) => {
  test.setTimeout(120_000)
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  const executable = await cliExecutable()
  const adminCookie = await ensureAdminSession(page, request, root)
  const owner = await createActiveRegularUser(request, root, adminCookie, 'skills-cli')
  const keyResponse = await request.post(`${root}/api/v1/admin/api-keys`, {
    headers: { cookie: adminCookie },
    data: {
      name: `Skills CLI ${Date.now()}`,
      owner_kind: 'user',
      owner_user_id: owner.id,
      owner_team_id: null,
      owner_service_account_id: null,
      model_grant_mode: 'all',
      model_keys: [],
    },
  })
  expect(keyResponse.status()).toBe(200)
  const key = (await keyResponse.json()) as {
    data: { api_key: { id: string }; raw_key: string }
  }
  const temporary = await mkdtemp(path.join(tmpdir(), 'oceans-skills-cli-'))

  async function run<T>(...args: string[]): Promise<T> {
    try {
      const { stdout } = await execFileAsync(
        executable,
        ['--url', root, '--json', 'skills', ...args],
        {
          env: { ...process.env, OCEANS_API_KEY: key.data.raw_key },
          timeout: 30_000,
          maxBuffer: 4 * 1024 * 1024,
        },
      )
      return JSON.parse(stdout) as T
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      throw new Error(message.replaceAll(key.data.raw_key, '[redacted]'))
    }
  }

  try {
    const namespace = `cli-${Date.now().toString(36)}`
    expect((await run<{ handle: string }>('namespace', namespace)).handle).toBe(namespace)
    const address = `${namespace}/cli-review`
    const source = path.join(temporary, 'cli-review')
    await mkdir(source)
    await writeFile(path.join(source, 'SKILL.md'), skillInstructions(1))
    const first = await run<UploadResult>('upload', source)
    expect(first.uploaded_version).toBe(1)
    const listed = await run<SkillDetail['skill'][]>('list', '--namespace', namespace)
    expect(listed.map((skill) => skill.id)).toContain(first.skill.id)
    expect((await run<Preview>('show', address)).version.instructions).toContain('version 1')

    const installed = await run<{ path: string }>(
      'install',
      address,
      '--directory',
      path.join(temporary, 'installed'),
    )
    const record = JSON.parse(
      await readFile(path.join(installed.path, '.oceans-skill-lock.json'), 'utf8'),
    )
    expect(Object.keys(record).sort()).toEqual(['namespace', 'sha256', 'skill_id', 'version'])
    expect(record.skill_id).toBe(first.skill.id)
    await writeFile(path.join(installed.path, 'SKILL.md'), skillInstructions(2))
    const second = await run<UploadResult>('upload', installed.path)
    expect(second.uploaded_version).toBe(2)
    expect(second.skill.default_version).toBe(1)

    await run<SkillDetail>('set-default', address, '2')
    const selected = await run<Preview>('show', address)
    expect(selected.version.instructions).toContain('version 2')
    expect(selected.version.files.map((file) => file.path)).not.toContain('.oceans-skill-lock.json')
    const archivePath = path.join(temporary, 'download.zip')
    const downloaded = await run<{ version: number }>('download', address, '--output', archivePath)
    expect(downloaded.version).toBe(2)
    expect(
      createHash('sha256')
        .update(await readFile(archivePath))
        .digest('hex'),
    ).toBe(selected.version.version.sha256)
  } finally {
    await request.post(`${root}/api/v1/admin/api-keys/${key.data.api_key.id}/revoke`, {
      headers: { cookie: adminCookie },
    })
    await rm(temporary, { recursive: true, force: true })
  }
})
