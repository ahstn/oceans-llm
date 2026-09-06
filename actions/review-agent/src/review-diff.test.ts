import { expect, test } from 'bun:test'
import { execFileSync } from 'node:child_process'
import { chmodSync, mkdtempSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { loadReviewDiff } from './review-diff'
import type { PullRequestContext } from './types'

test('counts deletions, binary changes and mode changes separately from anchors; streams large diffs', async () => {
  const workspace = mkdtempSync(join(tmpdir(), 'review-diff-test-'))
  const git = (...args: string[]) =>
    execFileSync('git', args, { cwd: workspace, encoding: 'utf8' }).trim()
  try {
    git('init', '-q')
    git('config', 'user.email', 'test@example.test')
    git('config', 'user.name', 'Test')
    writeFileSync(join(workspace, 'deleted.txt'), 'old\n')
    writeFileSync(join(workspace, 'binary.dat'), Buffer.from([0, 1]))
    writeFileSync(join(workspace, 'mode.sh'), '#!/bin/sh\n')
    git('add', '.')
    git('commit', '-qm', 'base')
    const base = git('rev-parse', 'HEAD')
    rmSync(join(workspace, 'deleted.txt'))
    writeFileSync(join(workspace, 'binary.dat'), Buffer.from([0, 2]))
    chmodSync(join(workspace, 'mode.sh'), 0o755)
    // A single large added line crosses the old execFileSync stdout limit.
    writeFileSync(join(workspace, 'large.txt'), `${'x'.repeat(9 * 1024 * 1024)}\n`)
    git('add', '.')
    git('commit', '-qm', 'head')
    const context: PullRequestContext = {
      repository: { provider: 'github', owner: 'test', name: 'repo', full_name: 'test/repo' },
      pullRequest: {
        pr_number: 1,
        is_draft: false,
        base_sha: base,
        head_sha: git('rev-parse', 'HEAD'),
        head_repository_full_name: 'test/repo',
        base_repository_full_name: 'test/repo',
      },
    }
    const diffPath = join(workspace, 'review.diff')
    const diff = await loadReviewDiff(workspace, context, diffPath)
    expect(diff.filesChanged).toBe(4)
    expect([...diff.anchors.keys()]).toEqual(['large.txt'])
    expect([...diff.anchors.get('large.txt')!]).toEqual([1])
    expect(statSync(diffPath).size).toBeGreaterThan(8 * 1024 * 1024)
  } finally {
    rmSync(workspace, { recursive: true, force: true })
  }
})
