import { spawnSync } from 'node:child_process'

// tsrs publishes binaries for these platforms only (the @tsrs/* optional
// dependencies in bun.lock). Everywhere else, check with TypeScript 7.
const tsrsPlatforms = new Set(['darwin-arm64', 'linux-x64', 'linux-arm64'])
const script = tsrsPlatforms.has(`${process.platform}-${process.arch}`)
  ? 'typecheck'
  : 'typecheck:ts7'

const result = spawnSync('bun', ['run', script], {
  stdio: 'inherit',
  shell: process.platform === 'win32',
})
process.exit(result.status ?? 1)
