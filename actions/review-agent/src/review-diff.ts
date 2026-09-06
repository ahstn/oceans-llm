import { execFileSync } from 'node:child_process'
import { closeSync, createReadStream, openSync } from 'node:fs'
import { createInterface } from 'node:readline'
import type { PullRequestContext } from './types'

export interface ReviewDiff {
  filesChanged: number
  anchors: Map<string, Set<number>>
}

export async function loadReviewDiff(
  workspace: string,
  context: PullRequestContext,
  diffPath: string,
): Promise<ReviewDiff> {
  const { base_sha: base, head_sha: head } = context.pullRequest
  if (!base || !head || !/^[a-f0-9]{40,64}$/.test(base) || !/^[a-f0-9]{40,64}$/.test(head)) {
    throw new Error('Review requires full base and head commit SHAs')
  }
  const descriptor = openSync(diffPath, 'w', 0o600)
  try {
    execFileSync(
      'git',
      [
        '-c',
        'core.quotePath=false',
        'diff',
        '--no-color',
        '--src-prefix=a/',
        '--dst-prefix=b/',
        '--no-ext-diff',
        '--no-textconv',
        '--unified=0',
        `${base}...${head}`,
        '--',
      ],
      { cwd: workspace, stdio: ['ignore', descriptor, 'pipe'] },
    )
  } finally {
    closeSync(descriptor)
  }
  // Count Git's changed paths independently of commentable RIGHT-side hunks.
  const paths = execFileSync(
    'git',
    ['diff', '--no-ext-diff', '--no-textconv', '--name-only', '-z', `${base}...${head}`, '--'],
    { cwd: workspace, maxBuffer: 8 * 1024 * 1024 },
  )
  const lines = createInterface({ input: createReadStream(diffPath), crlfDelay: Infinity })
  const parser = new AnchorParser()
  for await (const line of lines) parser.add(line)
  return {
    filesChanged: paths.reduce((count, byte) => count + Number(byte === 0), 0),
    anchors: parser.anchors,
  }
}

export function parseDiffAnchors(text: string): Map<string, Set<number>> {
  const parser = new AnchorParser()
  for (const line of text.split('\n')) parser.add(line)
  return parser.anchors
}

class AnchorParser {
  readonly anchors = new Map<string, Set<number>>()
  private path: string | undefined

  add(line: string): void {
    if (line.startsWith('diff --git ')) this.path = undefined
    if (line.startsWith('+++ b/')) {
      this.path = line.slice(6)
      this.anchors.set(this.path, new Set())
    }
    const match = /^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@/.exec(line)
    if (!match || !this.path) return
    const start = Number(match[1])
    const count = match[2] === undefined ? 1 : Number(match[2])
    for (let n = start; n < start + count; n++) this.anchors.get(this.path)!.add(n)
  }
}
