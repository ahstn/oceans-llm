import { execFileSync } from 'node:child_process'
import { closeSync, createReadStream, openSync } from 'node:fs'
import { createInterface } from 'node:readline'
import type { PullRequestContext } from './types'

export interface ReviewDiff {
  additions: number
  deletions: number
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
    additions: parser.additions,
    deletions: parser.deletions,
  }
}

export function parseDiffAnchors(text: string): Map<string, Set<number>> {
  const parser = new AnchorParser()
  for (const line of text.split('\n')) parser.add(line)
  return parser.anchors
}

class AnchorParser {
  readonly anchors = new Map<string, Set<number>>()
  additions = 0
  deletions = 0
  private path: string | undefined
  private inHunk = false

  add(line: string): void {
    if (line.startsWith('diff --git ')) {
      this.path = undefined
      this.inHunk = false
    }
    if (!this.inHunk && line.startsWith('+++ ')) {
      const destination = decodeGitPath(line.slice(4).split('\t')[0])
      this.path = destination.startsWith('b/') ? destination.slice(2) : undefined
      if (this.path) this.anchors.set(this.path, new Set())
    }
    const match = /^@@ -\d+(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(line)
    if (!match) return
    this.inHunk = true
    const start = Number(match[2])
    const count = match[3] === undefined ? 1 : Number(match[3])
    // loadReviewDiff always requests zero context, so hunk ranges are changed lines.
    this.additions += count
    this.deletions += match[1] === undefined ? 1 : Number(match[1])
    if (this.path) {
      for (let n = start; n < start + count; n++) this.anchors.get(this.path)!.add(n)
    }
  }
}

function decodeGitPath(path: string): string {
  if (!path.startsWith('"')) return path
  // Git quotes bytes using C escapes, including octal UTF-8 bytes when quotePath is true.
  const source = Buffer.from(path.slice(1, -1))
  const decoded: number[] = []
  const escapes: Record<number, number> = {
    97: 7,
    98: 8,
    116: 9,
    110: 10,
    118: 11,
    102: 12,
    114: 13,
    34: 34,
    92: 92,
  }
  for (let i = 0; i < source.length; i++) {
    let byte = source[i]
    if (byte !== 92) {
      decoded.push(byte)
      continue
    }
    byte = source[++i]
    if (byte >= 48 && byte <= 55) {
      let value = byte - 48
      for (let digits = 1; digits < 3 && source[i + 1] >= 48 && source[i + 1] <= 55; digits++) {
        value = value * 8 + source[++i] - 48
      }
      byte = value
    } else {
      byte = escapes[byte]
      if (byte === undefined) throw new Error('Invalid Git path escape')
    }
    decoded.push(byte)
  }
  return Buffer.from(decoded).toString('utf8')
}
