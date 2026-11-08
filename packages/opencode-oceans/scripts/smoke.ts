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

async function command(args: string[], cwd: string) {
  const child = Bun.spawn(args, { cwd, stdout: 'pipe', stderr: 'pipe' })
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

if (import.meta.main) {
  const root = await mkdtemp(path.join(tmpdir(), 'opencode-oceans-smoke-'))
  const { server, requests } = gateway()
  let host: Subprocess<'pipe', 'pipe', 'pipe'> | undefined
  try {
    const archive = path.join(root, 'plugin.tgz')
    await command([process.execPath, 'pm', 'pack', '--filename', archive], packageRoot)
    await command(['tar', '-xzf', archive, '-C', root], packageRoot)
    await mkdir(path.join(root, 'workspace'))
    const configDir = path.join(root, 'config')
    await mkdir(configDir)
    await Bun.write(
      path.join(configDir, 'gateways.yml'),
      `providers:\n  oceans:\n    baseUrl: ${server.url.origin}\n    apiKey: OCEANS_SMOKE_KEY\n    discovery: {type: litellm}\n`,
    )
    // Execute the upstream installer with Bun; it links the already-installed native host binary.
    await command([process.execPath, path.join(cliRoot, 'postinstall.mjs')], packageRoot)
    const binary = path.join(cliRoot, 'bin', 'opencode.exe')
    assert.equal((await command([binary, '--version'], packageRoot)).trim(), 'opencode v2.2.6')
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
      OPENCODE_PASSWORD: password,
      OCEANS_SMOKE_KEY: 'synthetic-key',
      OPENCODE_CONFIG_DIR: configDir,
      OPENCODE_DISABLE_AUTOUPDATE: 'true',
      OPENCODE_DISABLE_CLAUDE_CODE: 'true',
      OPENCODE_DISABLE_FILEWATCHER: 'true',
      OPENCODE_DISABLE_FREE_MODELS: 'true',
      OPENCODE_DISABLE_MODELS_FETCH: 'true',
      OPENCODE_CONFIG_CONTENT: JSON.stringify({
        plugins: [{ package: path.join(root, 'package'), options: { configDir } }],
        agents: { build: { steps: 1 } },
      }),
    }
    host = Bun.spawn([binary, 'serve', '--stdio', '--port', '0'], {
      cwd: path.join(root, 'workspace'),
      env,
      stdin: 'pipe',
      stdout: 'pipe',
      stderr: 'pipe',
    })
    const stderr = new Response(host.stderr).text()
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
    console.log('PASS packed source package loaded by OpenCode 2.2.6; no external model calls')
  } finally {
    host?.kill()
    if (host) await host.exited
    await server.stop(true)
    await rm(root, { recursive: true, force: true })
  }
}
