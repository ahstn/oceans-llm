import assert from 'node:assert/strict'
import { mkdtemp, mkdir, rm } from 'node:fs/promises'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'
import type { Subprocess } from 'bun'
import { OpenCode, type OpenCodeClient } from '@opencode/client'
import { Schema } from 'effect'
import { record } from '../src/config'
import { gateway, modelIDs, png, type CapturedRequest } from './gateway'

const packageRoot = path.resolve(import.meta.dir, '..')
const require = createRequire(import.meta.url)
const cliRoot = path.dirname(require.resolve('@opencode/cli/package.json'))

async function command(args: string[], cwd: string, env: Record<string, string | undefined>) {
  const child = Bun.spawn(args, { cwd, env, stdout: 'pipe', stderr: 'pipe' })
  const [stdout, stderr, code] = await Promise.all([
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
    child.exited,
  ])
  assert.equal(code, 0, `${args.join(' ')} failed: ${stderr}`)
  return stdout
}

async function exercise(client: OpenCodeClient, requests: CapturedRequest[], directory: string) {
  const models = (await client.model.list({ location: { directory } })).data.filter(
    (model) => model.providerID === 'oceans',
  )
  assert.deepEqual(models.map((model) => model.id).sort(), [...modelIDs].sort())
  assert.equal(
    models.find((model) => model.id === 'claude-fixture')?.package,
    '@opencode/ai/providers/anthropic-compatible',
  )
  await Promise.all(
    modelIDs.map(async (id) => {
      const session = await client.session.create({
        title: `Smoke ${id}`,
        location: { directory },
        model: { providerID: 'oceans', id, variant: 'high' },
      })
      await client.session.prompt({
        sessionID: session.id,
        text: 'Describe these synthetic attachments. Do not use tools.',
        files: [
          { name: 'pixel.png', uri: `data:image/png;base64,${png}`, source: { type: 'inline' } },
          {
            name: 'sample.pdf',
            uri: `data:application/pdf;base64,${Buffer.from('%PDF-1.4\n%%EOF').toString('base64')}`,
            source: { type: 'inline' },
          },
        ],
      })
      await client.session.wait({ sessionID: session.id }, { signal: AbortSignal.timeout(30_000) })
      const messages = await client.message.list({ sessionID: session.id })
      const assistant = messages.data.find((message) => message.type === 'assistant')
      assert.ok(
        assistant && assistant.type === 'assistant',
        `No assistant message for ${id}: ${JSON.stringify(messages)}`,
      )
      assert.ok(assistant.tokens, `No usage for ${id}`)
      assert.equal(assistant.tokens.input, 70)
      assert.equal(assistant.tokens.cache.read, 30)
      assert.equal(assistant.tokens.output + assistant.tokens.reasoning, 20)
      assert.equal(assistant.tokens.cache.write, id === 'claude-fixture' ? 10 : 0)
      const expected =
        id === 'free-fixture'
          ? 0
          : id === 'billed-fixture'
            ? 0.123
            : id === 'claude-fixture'
              ? 0.0005565
              : 0.000519
      assert.ok(
        Math.abs((assistant.cost ?? -1) - expected) < 1e-10,
        `Cost ${id}: ${assistant.cost} != ${expected}`,
      )
      const request = requests.find((item) => item.body.model === id)
      assert.ok(request, `No inference request for ${id}`)
      const payload = JSON.stringify(request.body)
      assert.ok(payload.includes(png), `Image missing for ${id}`)
      if (id === 'claude-fixture') {
        assert.equal(request.path, '/v1/messages')
        assert.deepEqual(request.body.thinking, { type: 'adaptive' })
        assert.deepEqual(request.body.output_config, { effort: 'high' })
        assert.ok(payload.includes('"type":"document"'), 'PDF missing in Messages')
      } else if (id === 'gpt-fixture') {
        assert.equal(request.path, '/v1/responses')
        assert.equal(record(request.body.reasoning).effort, 'high')
        assert.ok(payload.includes('"type":"input_file"'), 'PDF missing in Responses')
      } else {
        assert.equal(request.path, '/v1/chat/completions')
        assert.equal(request.body.reasoning_effort, 'high')
        assert.ok(payload.includes('"type":"file"'), 'PDF missing in Chat')
      }
      console.log(
        `PASS ${id}: ${request.path}, image/PDF, high reasoning, tokens/cache, session cost $${assistant.cost}`,
      )
      await client.session.remove({ sessionID: session.id })
    }),
  )
}

export async function readReadyLine(stream: ReadableStream<Uint8Array>): Promise<string> {
  const decoder = new TextDecoder()
  let line = ''
  for await (const chunk of stream.values({ preventCancel: true })) {
    line += decoder.decode(chunk, { stream: true })
    const newline = line.indexOf('\n')
    if (newline !== -1) return line.slice(0, newline)
  }
  throw new Error('Host readiness stream ended before a complete line')
}

export function publishedVersion(args: string[]): string | undefined {
  if (args.length === 0) return undefined
  if (
    args.length !== 2 ||
    args[0] !== '--published' ||
    !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?![\s\S])/.test(args[1] ?? '')
  ) {
    throw new Error('Usage: smoke.ts [--published <exact stable X.Y.Z version>]')
  }
  return args[1]
}

async function assertInstalledVersion(root: string, version: string) {
  const manifests = new Bun.Glob('**/node_modules/@ahstn/opencode-oceans/package.json')
  let installed = 0
  for await (const manifest of manifests.scan({ cwd: root, dot: true, followSymlinks: true })) {
    const metadata = await Bun.file(path.join(root, manifest)).json()
    assert.equal(metadata.name, '@ahstn/opencode-oceans')
    assert.equal(metadata.version, version, `Unexpected installed plugin at ${manifest}`)
    installed++
  }
  assert.ok(installed > 0, 'OpenCode did not install the requested npm plugin')
}

if (import.meta.main) {
  const version = publishedVersion(process.argv.slice(2))
  const root = await mkdtemp(path.join(tmpdir(), 'opencode-oceans-smoke-'))
  const { server, requests } = gateway()
  let host: Subprocess<'pipe', 'pipe', 'pipe'> | undefined
  let stderr: Promise<string> | undefined
  let passed = false
  try {
    await mkdir(path.join(root, 'workspace'))
    const configDir = path.join(root, 'config')
    await mkdir(configDir)
    await Bun.write(
      path.join(configDir, 'gateways.yml'),
      `providers:\n  oceans:\n    baseUrl: ${server.url.origin}\n    apiKey: OCEANS_SMOKE_KEY\n    discovery: {type: litellm}\n`,
    )
    const password = crypto.randomUUID()
    const env = {
      PATH: process.env.PATH,
      TMPDIR: root,
      LANG: 'en_US.UTF-8',
      HOME: root,
      XDG_CONFIG_HOME: path.join(root, 'xdg-config'),
      XDG_DATA_HOME: path.join(root, 'data'),
      XDG_CACHE_HOME: path.join(root, 'cache'),
      XDG_STATE_HOME: path.join(root, 'state'),
      NPM_CONFIG_USERCONFIG: path.join(root, '.npmrc'),
      NPM_CONFIG_GLOBALCONFIG: path.join(root, 'global.npmrc'),
      NPM_CONFIG_REGISTRY: 'https://registry.npmjs.org',
      BUN_CONFIG_NO_CLEAR_TERMINAL: '1',
      OPENCODE_PASSWORD: password,
      OCEANS_SMOKE_KEY: 'synthetic-key',
      OPENCODE_CONFIG_DIR: configDir,
      OPENCODE_DISABLE_AUTOUPDATE: 'true',
      OPENCODE_DISABLE_CLAUDE_CODE: 'true',
      OPENCODE_DISABLE_FILEWATCHER: 'true',
      OPENCODE_DISABLE_FREE_MODELS: 'true',
      OPENCODE_DISABLE_MODELS_FETCH: 'true',
      OPENCODE_CONFIG_CONTENT: JSON.stringify({
        plugins: [
          {
            package: version ? `@ahstn/opencode-oceans@${version}` : path.join(root, 'package'),
            options: { configDir },
          },
        ],
        agents: { build: { steps: 1 } },
      }),
    }
    await Bun.write(path.join(root, '.npmrc'), 'registry=https://registry.npmjs.org/\n')
    await Bun.write(path.join(root, 'global.npmrc'), '')
    await Bun.write(
      path.join(root, '.bunfig.toml'),
      '[install]\nregistry = "https://registry.npmjs.org"\n',
    )
    if (!version) {
      const archive = path.join(root, 'plugin.tgz')
      await command([process.execPath, 'pm', 'pack', '--filename', archive], packageRoot, env)
      await command(['tar', '-xzf', archive, '-C', root], packageRoot, env)
    }
    // Execute the upstream installer with Bun; it links the already-installed native host binary.
    await command([process.execPath, path.join(cliRoot, 'postinstall.mjs')], root, env)
    const binary = path.join(cliRoot, 'bin', 'opencode.exe')
    assert.equal((await command([binary, '--version'], root, env)).trim(), 'opencode v2.3.0')
    if (version) {
      await command([binary, 'plugin', 'add', `@ahstn/opencode-oceans@${version}`], root, env)
    }
    host = Bun.spawn(
      [binary, 'serve', '--stdio', '--port', '0', '--print-logs', '--log-level', 'debug'],
      {
        cwd: path.join(root, 'workspace'),
        env,
        stdin: 'pipe',
        stdout: 'pipe',
        stderr: 'pipe',
      },
    )
    stderr = new Response(host.stderr).text()
    const readiness = await Promise.race([
      readReadyLine(host.stdout),
      host.exited.then(async (code) => {
        throw new Error(`Host exited ${code}: ${await stderr}`)
      }),
      delay(30_000, undefined, { ref: false }).then(() => {
        throw new Error('Host readiness timeout')
      }),
    ])
    const { url } = Schema.decodeUnknownSync(Schema.Struct({ url: Schema.String }))(
      JSON.parse(readiness),
    )
    const client = OpenCode.make({
      baseUrl: url,
      headers: { authorization: `Basic ${Buffer.from(`opencode:${password}`).toString('base64')}` },
    })
    await exercise(client, requests, path.join(root, 'workspace'))
    if (version) await assertInstalledVersion(root, version)
    passed = true
    console.log(
      `PASS ${version ? `published @ahstn/opencode-oceans@${version}` : 'packed source package'} loaded by OpenCode 2.3.0; no external model calls`,
    )
  } finally {
    host?.kill()
    if (host) await host.exited
    if (!passed && stderr) console.error(await stderr)
    await server.stop(true)
    await rm(root, { recursive: true, force: true })
  }
}
