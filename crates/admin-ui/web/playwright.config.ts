import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'playwright/test'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const repoRoot = path.resolve(__dirname, '../../..')

const gatewayPort = process.env.E2E_GATEWAY_PORT ?? '38080'
const uiPort = process.env.E2E_UI_PORT ?? '33001'
const upstreamPort = process.env.E2E_UPSTREAM_PORT ?? '38081'
const baseURL = process.env.E2E_BASE_URL ?? `http://127.0.0.1:${gatewayPort}`

export default defineConfig({
  testDir: './e2e',
  testMatch: /.*\.e2e\.ts/,
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  timeout: 60_000,
  reporter: [['list'], ['html', { open: 'never', outputFolder: 'playwright-report' }]],
  outputDir: 'test-results',
  use: {
    baseURL,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },
  webServer: {
    command: path.join(repoRoot, 'scripts', 'start-e2e-stack.sh'),
    cwd: repoRoot,
    url: `${baseURL}/readyz`,
    reuseExistingServer: false,
    timeout: 180_000,
    stdout: 'pipe',
    stderr: 'pipe',
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
  },
})
