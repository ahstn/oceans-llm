import { describe, expect, test } from 'bun:test'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { sandboxCommand } from './worker-sandbox'

function sandboxArgs(gitMetadata: 'directory' | 'file'): string[] {
  const root = mkdtempSync(join(tmpdir(), 'review-sandbox-test-'))
  const actionRoot = join(root, 'action')
  const workspace = join(root, 'workspace')
  const workerTemp = join(root, 'worker')
  mkdirSync(actionRoot)
  mkdirSync(workspace)
  mkdirSync(workerTemp)
  const gitPath = join(workspace, '.git')
  if (gitMetadata === 'directory') {
    mkdirSync(gitPath)
    writeFileSync(join(gitPath, 'config'), 'CHECKOUT_TOKEN_MUST_NOT_REACH_MODEL')
  } else {
    writeFileSync(gitPath, 'gitdir: /credential-bearing/metadata')
  }

  try {
    return sandboxCommand(actionRoot, workspace, workerTemp, ['worker.js']).args
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
}

describe('review worker sandbox', () => {
  test('overlays checkout Git directories after mounting the readable source', () => {
    const args = sandboxArgs('directory')
    const workspaceMount = args.findIndex(
      (arg, index) => arg.endsWith('/workspace') && args[index - 2] === '--ro-bind',
    )
    const gitOverlay = args.indexOf('--tmpfs')

    expect(workspaceMount).toBeGreaterThanOrEqual(0)
    expect(args[workspaceMount - 2]).toBe('--ro-bind')
    expect(gitOverlay).toBeGreaterThan(workspaceMount)
    expect(args[gitOverlay + 1]?.endsWith('/workspace/.git')).toBe(true)
  })

  test('replaces worktree Git files with an empty device', () => {
    const args = sandboxArgs('file')
    const gitPathIndex = args.findIndex((arg) => arg.endsWith('/workspace/.git'))

    expect(gitPathIndex).toBeGreaterThan(1)
    expect(args.slice(gitPathIndex - 2, gitPathIndex + 1)).toEqual([
      '--ro-bind',
      '/dev/null',
      args[gitPathIndex],
    ])
  })
})
