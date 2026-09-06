import { execFileSync } from 'node:child_process'
import type { PullRequestContext } from './types'

export interface ReviewDiff {
  text: string
  anchors: Map<string, Set<number>>
}

export function loadReviewDiff(workspace: string, context: PullRequestContext): ReviewDiff {
  const { base_sha: base, head_sha: head } = context.pullRequest
  if (!base || !head || !/^[a-f0-9]{40,64}$/.test(base) || !/^[a-f0-9]{40,64}$/.test(head)) {
    throw new Error('Review requires full base and head commit SHAs')
  }
  const text = execFileSync(
    'git',
    [
      '-c',
      'core.quotePath=false',
      'diff',
      '--no-ext-diff',
      '--no-textconv',
      '--unified=0',
      `${base}...${head}`,
      '--',
    ],
    { cwd: workspace, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 },
  )
  return { text, anchors: parseDiffAnchors(text) }
}

export function parseDiffAnchors(text: string): Map<string, Set<number>> {
  const anchors = new Map<string, Set<number>>()
  let path: string | undefined
  for (const line of text.split('\n')) {
    if (line.startsWith('diff --git ')) path = undefined
    if (line.startsWith('+++ b/')) {
      path = line.slice(6)
      anchors.set(path, new Set())
    }
    const match = /^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@/.exec(line)
    if (!match || !path) continue
    const start = Number(match[1])
    const count = match[2] === undefined ? 1 : Number(match[2])
    for (let n = start; n < start + count; n++) anchors.get(path)!.add(n)
  }
  return anchors
}
