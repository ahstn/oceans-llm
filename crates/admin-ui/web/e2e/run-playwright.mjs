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

const playwrightRoot = path.dirname(require.resolve('playwright/package.json'))
const playwrightCli = path.join(playwrightRoot, 'cli.js')

async function runPlaywright(testArgs, extraEnv, requested = {}) {
  const gatewayPort = await reservePort(requested.gatewayPort)
  const uiPort = await reservePort(requested.uiPort)
  const upstreamPort = await reservePort(requested.upstreamPort)
  const baseURL = requested.baseURL ?? `http://127.0.0.1:${gatewayPort}`
  const child = spawn(process.execPath, [playwrightCli, 'test', ...testArgs], {
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
      ...extraEnv,
    },
  })

  await new Promise((resolve, reject) => {
    child.on('exit', (code, signal) => {
      if (signal) {
        reject(new Error(`Playwright exited after signal ${signal}`))
      } else if (code === 0) {
        resolve()
      } else {
        reject(new Error(`Playwright exited with status ${code ?? 1}`))
      }
    })
  })
}

await runPlaywright(
  [],
  { E2E_PERMISSION_SCENARIO: 'default' },
  {
    gatewayPort: process.env.E2E_GATEWAY_PORT,
    uiPort: process.env.E2E_UI_PORT,
    upstreamPort: process.env.E2E_UPSTREAM_PORT,
    baseURL: process.env.E2E_BASE_URL,
  },
)
await runPlaywright(['admin-permission-overrides.e2e.ts'], {
  E2E_PERMISSION_SCENARIO: 'overrides',
})
