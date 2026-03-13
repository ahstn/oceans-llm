import { spawn } from 'node:child_process'
import { createRequire } from 'node:module'
import { createServer } from 'node:net'
import path from 'node:path'

const require = createRequire(import.meta.url)

async function reservePort(explicitPort) {
  if (explicitPort) {
    return explicitPort
  }

  const server = createServer()
  await new Promise((resolve, reject) => {
    server.listen(0, '127.0.0.1', resolve)
    server.once('error', reject)
  })

  const address = server.address()
  if (!address || typeof address === 'string') {
    server.close()
    throw new Error('Failed to allocate an E2E port')
  }

  const port = String(address.port)
  await new Promise((resolve, reject) => {
    server.close((error) => {
      if (error) {
        reject(error)
        return
      }

      resolve()
    })
  })

  return port
}

const gatewayPort = await reservePort(process.env.E2E_GATEWAY_PORT)
const uiPort = await reservePort(process.env.E2E_UI_PORT)
const upstreamPort = await reservePort(process.env.E2E_UPSTREAM_PORT)
const baseURL = process.env.E2E_BASE_URL ?? `http://127.0.0.1:${gatewayPort}`
const playwrightRoot = path.dirname(require.resolve('playwright/package.json'))
const playwrightCli = path.join(playwrightRoot, 'cli.js')

const child = spawn(process.execPath, [playwrightCli, 'test'], {
  stdio: 'inherit',
  env: {
    ...process.env,
    E2E_GATEWAY_PORT: gatewayPort,
    E2E_UI_PORT: uiPort,
    E2E_UPSTREAM_PORT: upstreamPort,
    E2E_BASE_URL: baseURL,
    E2E_GATEWAY_API_KEY: process.env.E2E_GATEWAY_API_KEY ?? 'gwk_e2e.secret-value',
    E2E_ADMIN_EMAIL: process.env.E2E_ADMIN_EMAIL ?? 'admin@local',
    E2E_ADMIN_PASSWORD: process.env.E2E_ADMIN_PASSWORD ?? 'admin',
    E2E_ADMIN_NEW_PASSWORD: process.env.E2E_ADMIN_NEW_PASSWORD ?? 's3cur3-passw0rd',
  },
})

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal)
    return
  }

  process.exit(code ?? 1)
})
