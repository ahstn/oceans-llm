// Sample data for the attribution concept mockups. It is shaped like the stored attribution rows
// (commits with AI and human lines, the agent sessions behind them, merged pull requests, and the
// gateway cost those sessions made), plus the fields a concept needs that Oceans does not record
// yet, such as line durability and reverts. Every value comes from a seeded generator, so server
// and browser render the same numbers and the mockups stay stable between screenshots.

export const AGENT_TOOLS = ['claude', 'codex', 'pi', 'cursor', 'opencode'] as const
export type AgentTool = (typeof AGENT_TOOLS)[number]

export const AGENT_TOOL_LABELS: Record<AgentTool, string> = {
  claude: 'Claude Code',
  codex: 'Codex',
  pi: 'Pi',
  cursor: 'Cursor',
  opencode: 'OpenCode',
}

/** The icon key `AgentHarnessIcon` understands, where one exists. */
export const AGENT_TOOL_ICON_KEYS: Record<AgentTool, string> = {
  claude: 'claudecode',
  codex: 'codex',
  pi: 'pi',
  cursor: 'cursor',
  opencode: 'opencode',
}

export type MockUser = {
  id: string
  name: string
  team: string
  /** Base chance that a commit carries AI lines. */
  aiAffinity: number
  commitsPerDay: number
  tools: Partial<Record<AgentTool, number>>
  /** Median working hour, UTC. */
  peakHour: number
}

export const MOCK_USERS: MockUser[] = [
  {
    id: 'u-priya',
    name: 'Priya Natarajan',
    team: 'Platform',
    aiAffinity: 0.82,
    commitsPerDay: 3.2,
    tools: { claude: 0.6, codex: 0.3, pi: 0.1 },
    peakHour: 10,
  },
  {
    id: 'u-marcus',
    name: 'Marcus Chen',
    team: 'Platform',
    aiAffinity: 0.64,
    commitsPerDay: 2.6,
    tools: { codex: 0.7, claude: 0.3 },
    peakHour: 15,
  },
  {
    id: 'u-lena',
    name: 'Lena Vogel',
    team: 'Billing',
    aiAffinity: 0.48,
    commitsPerDay: 2.1,
    tools: { cursor: 0.65, claude: 0.35 },
    peakHour: 9,
  },
  {
    id: 'u-tomas',
    name: 'Tomás Reyes',
    team: 'Billing',
    aiAffinity: 0.71,
    commitsPerDay: 2.8,
    tools: { pi: 0.55, opencode: 0.45 },
    peakHour: 13,
  },
  {
    id: 'u-rosa',
    name: 'Rosa Duarte',
    team: 'Web',
    aiAffinity: 0.38,
    commitsPerDay: 1.9,
    tools: { cursor: 0.5, claude: 0.5 },
    peakHour: 11,
  },
  {
    id: 'u-sam',
    name: 'Sam Okafor',
    team: 'Web',
    aiAffinity: 0.57,
    commitsPerDay: 2.3,
    tools: { claude: 0.8, opencode: 0.2 },
    peakHour: 16,
  },
]

export type MockRepository = {
  name: string
  /** Multiplies the chance of AI lines: codebases agents work well in score higher. */
  aiFit: number
  /** How much more of a session's work is discarded before commit. */
  thrash: number
  /** Share of merged AI lines still unchanged after 30 days. */
  durability: number
  weights: Record<string, number>
}

export const MOCK_REPOSITORIES: MockRepository[] = [
  {
    name: 'github.com/acme/gateway',
    aiFit: 1.15,
    thrash: 0.9,
    durability: 0.86,
    weights: { Platform: 0.7, Billing: 0.1, Web: 0.1 },
  },
  {
    name: 'github.com/acme/web-console',
    aiFit: 1.2,
    thrash: 1,
    durability: 0.78,
    weights: { Platform: 0.1, Billing: 0.1, Web: 0.75 },
  },
  {
    name: 'github.com/acme/billing-core',
    aiFit: 0.8,
    thrash: 1.6,
    durability: 0.64,
    weights: { Platform: 0.05, Billing: 0.7, Web: 0.05 },
  },
  {
    name: 'github.com/acme/infra-live',
    aiFit: 0.65,
    thrash: 2.1,
    durability: 0.57,
    weights: { Platform: 0.25, Billing: 0.1, Web: 0.05 },
  },
  {
    name: 'github.com/acme/mobile-app',
    aiFit: 1,
    thrash: 1.2,
    durability: 0.73,
    weights: { Platform: 0, Billing: 0.05, Web: 0.3 },
  },
]

/** Gateway model keys each harness sends, with the share of its sessions. */
const TOOL_MODELS: Record<AgentTool, [string, number][]> = {
  claude: [
    ['claude-sonnet-5-5', 0.7],
    ['claude-opus-5-5', 0.3],
  ],
  codex: [
    ['gpt-6-astra', 0.75],
    ['gpt-6-mini', 0.25],
  ],
  pi: [
    ['deepseek-v4.1-flash', 0.6],
    ['claude-sonnet-5-5', 0.4],
  ],
  cursor: [
    ['claude-sonnet-5-5', 0.55],
    ['gemini-3-pro', 0.45],
  ],
  opencode: [
    ['qwen-4-coder', 0.6],
    ['deepseek-v4.1-flash', 0.4],
  ],
}

/** Average priced cost of one request, in ten-thousandths of a dollar. */
const MODEL_REQUEST_COST: Record<string, number> = {
  'claude-opus-5-5': 980,
  'claude-sonnet-5-5': 360,
  'gpt-6-astra': 520,
  'gpt-6-mini': 70,
  'deepseek-v4.1-flash': 22,
  'gemini-3-pro': 310,
  'qwen-4-coder': 40,
}

/** Agent requests carry long contexts; this lifts the per-request averages to agent-session levels. */
const COST_SCALE = 3

/** Tokens per request, for the token efficiency views. */
const MODEL_REQUEST_TOKENS: Record<string, number> = {
  'claude-opus-5-5': 41_000,
  'claude-sonnet-5-5': 38_000,
  'gpt-6-astra': 33_000,
  'gpt-6-mini': 21_000,
  'deepseek-v4.1-flash': 29_000,
  'gemini-3-pro': 36_000,
  'qwen-4-coder': 27_000,
}

export type MockSession = {
  id: string
  userId: string
  repository: string
  tool: AgentTool
  model: string
  startedAt: number
  requests: number
  tokens: number
  costUsd10000: number
  /** Human turns that redirected the agent: the autonomy signal Git AI calls steering. */
  steeringTurns: number
  /** Lines the agent wrote, including lines it threw away before commit. */
  generatedLines: number
  /** Matched gateway traffic. Unjoined sessions bring lines but no requests or cost. */
  joined: boolean
  commitShas: string[]
}

export type MockCommit = {
  sha: string
  repository: string
  userId: string
  at: number
  branch: string
  isDefaultBranch: boolean
  aiLines: number
  humanLines: number
  /** Added lines with no attestation: unknown, not human. */
  unknownLines: number
  sessionIds: string[]
  tool: AgentTool | null
  model: string | null
  requests: number
  tokens: number
  costUsd10000: number
  pullRequestNumber: number | null
}

export type PullRequestState = 'merged' | 'open' | 'closed'

export type MockPullRequest = {
  repository: string
  number: number
  title: string
  userId: string
  headRef: string
  baseRef: string
  state: PullRequestState
  openedAt: number
  mergedAt: number | null
  firstCommitAt: number
  commitShas: string[]
  reviewComments: number
  /** Needs new data: a later PR reverted this one. */
  reverted: boolean
  /** Needs new data: AI lines changed in review, before merge. */
  aiLinesReworkedInReview: number
  /** Needs new data: merged AI lines still unchanged 30 days after merge. */
  aiLinesDurable30: number
}

export type MockDataset = {
  now: number
  commits: MockCommit[]
  sessions: MockSession[]
  pullRequests: MockPullRequest[]
}

export const DAY_MS = 86_400_000
export const HOUR_MS = 3_600_000
/** Fixed "today" so the mockups never drift: the end of 15 Nov 2026, UTC. */
export const MOCK_NOW = Date.UTC(2026, 10, 15, 23, 59)
const HISTORY_DAYS = 200

const PR_TITLES = [
  'Add idempotency keys to webhook retries',
  'Fix auth token refresh race',
  'Stream usage events to the warehouse',
  'Split provider config by vendor API',
  'Cache model catalogue lookups',
  'Retry 429s with jittered backoff',
  'Move invoice rendering to a worker',
  'Add per-team budget alerts',
  'Paginate the request log API',
  'Harden webhook signature checks',
  'Virtualise the audit log table',
  'Support Anthropic batch cancellation',
  'Drop the legacy session cookie',
  'Record MCP tool latency histograms',
  'Rewrite the rate limiter on token buckets',
  'Add dark mode charts to usage costs',
  'Terraform the read replica',
  'Fix flaky invoice rounding test',
  'Migrate push notifications to APNs v2',
  'Add OAuth device flow for the CLI',
  'Trim cold-start time on the edge proxy',
  'Validate guardrail YAML on save',
  'Backfill attribution for squash merges',
  'Bump Recharts and fix tooltip colours',
]

/** mulberry32: small, fast and seedable. */
function createRandom(seed: number) {
  let state = seed >>> 0
  return () => {
    state = (state + 0x6d2b79f5) >>> 0
    let t = state
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296
  }
}

type Random = () => number

function pick<T>(random: Random, entries: [T, number][]): T {
  const total = entries.reduce((sum, [, weight]) => sum + weight, 0)
  let roll = random() * total
  for (const [value, weight] of entries) {
    roll -= weight
    if (roll <= 0) return value
  }
  return entries[entries.length - 1][0]
}

/** Log-normal around a median: most values near it, a long tail of big changes. */
function logNormal(random: Random, median: number, spread = 0.8) {
  const u = Math.max(random(), 1e-9)
  const v = random()
  const normal = Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v)
  return median * Math.exp(normal * spread)
}

function hex(random: Random, length: number) {
  let out = ''
  while (out.length < length) out += Math.floor(random() * 16).toString(16)
  return out
}

function slug(title: string) {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 32)
}

type Branch = {
  repository: string
  userId: string
  size: number
  title: string
  commits: MockCommit[]
  /** Some work lands straight on the default branch with no pull request. */
  direct: boolean
}

// One linear pass over the history, so the workflow stays readable in sequence.
// oxlint-disable-next-line eslint/max-lines-per-function
function generate(): MockDataset {
  const random = createRandom(0x0ce4a5)
  const commits: MockCommit[] = []
  const sessions: MockSession[] = []
  const pullRequests: MockPullRequest[] = []
  const prNumbers = new Map<string, number>()
  const openBranches = new Map<string, Branch>()
  // Days start at UTC midnight, so weekday factors match the weekday commits land on.
  const start = Date.UTC(2026, 10, 16) - HISTORY_DAYS * DAY_MS
  let sessionSeq = 0

  function closeBranch(branch: Branch) {
    // Commits within a day are generated out of order; a branch reads in commit time.
    branch.commits.sort((a, b) => a.at - b.at)
    const first = branch.commits[0]
    const last = branch.commits[branch.commits.length - 1]
    if (!first || !last) return
    if (branch.direct) return
    const number = (prNumbers.get(branch.repository) ?? 400) + 1
    prNumbers.set(branch.repository, number)
    const aiLines = branch.commits.reduce((sum, commit) => sum + commit.aiLines, 0)
    const added = branch.commits.reduce(
      (sum, commit) => sum + commit.aiLines + commit.humanLines + commit.unknownLines,
      0,
    )
    const aiShare = added > 0 ? aiLines / added : 0
    const repo = MOCK_REPOSITORIES.find((entry) => entry.name === branch.repository)
    // Bigger, AI-heavier PRs wait longer for review.
    const reviewHours = logNormal(random, 5 + added / 220 + aiShare * 4, 0.9)
    const openedAt = first.at + (last.at - first.at) * 0.55 + HOUR_MS * 0.4
    const mergedAt = last.at + reviewHours * HOUR_MS
    const closedRoll = random()
    const state: PullRequestState =
      mergedAt > MOCK_NOW ? 'open' : closedRoll < 0.05 ? 'closed' : 'merged'
    const durability = Math.min(
      0.98,
      Math.max(0.2, (repo?.durability ?? 0.75) + (random() - 0.5) * 0.12 - aiShare * 0.36),
    )
    const headRef = `${branch.userId.slice(2)}/${slug(branch.title)}`
    for (const commit of branch.commits) {
      commit.pullRequestNumber = number
      commit.branch = headRef
    }
    pullRequests.push({
      repository: branch.repository,
      number,
      title: branch.title,
      userId: branch.userId,
      headRef,
      baseRef: 'main',
      state,
      openedAt,
      mergedAt: state === 'merged' ? mergedAt : null,
      firstCommitAt: first.at,
      commitShas: branch.commits.map((commit) => commit.sha),
      reviewComments: Math.round(logNormal(random, 2 + added / 300, 0.7)),
      reverted: state === 'merged' && random() < 0.015 + aiShare * 0.04 + (added > 900 ? 0.04 : 0),
      aiLinesReworkedInReview: Math.round(
        aiLines * Math.min(0.6, 0.04 + aiShare * 0.14 + random() * 0.12),
      ),
      aiLinesDurable30: Math.round(aiLines * durability),
    })
  }

  for (let day = 0; day < HISTORY_DAYS; day += 1) {
    const dayStart = start + day * DAY_MS
    const weekday = new Date(dayStart).getUTCDay()
    const weekdayFactor = weekday === 0 || weekday === 6 ? 0.12 : 1
    // Adoption ramps up through the history, so trends have somewhere to go.
    const ramp = 0.62 + 0.38 * (day / HISTORY_DAYS)

    for (const user of MOCK_USERS) {
      const expected = user.commitsPerDay * weekdayFactor * (0.6 + random() * 0.8)
      const count = Math.floor(expected + random())
      const daySessions = new Map<string, MockSession>()

      for (let index = 0; index < count; index += 1) {
        const repository = pick(
          random,
          MOCK_REPOSITORIES.map((repo) => [repo, repo.weights[user.team] ?? 0.05]),
        )
        const hour = Math.min(22, Math.max(6, user.peakHour + (random() - 0.5) * 9))
        const at = dayStart + hour * HOUR_MS + Math.floor(random() * 59) * 60_000
        const usesAi = random() < Math.min(0.97, user.aiAffinity * repository.aiFit * ramp)
        const tool = usesAi
          ? pick(random, Object.entries(user.tools) as [AgentTool, number][])
          : null
        // Agent commits run larger: LinearB sees AI-assisted PRs about 2.6x the size.
        const added = Math.round(logNormal(random, usesAi ? 95 : 55, 0.9)) + 2
        // Agentic commits land almost untouched; assisted ones mix in human edits.
        const aiShare = usesAi
          ? random() < 0.58
            ? 0.96 + random() * 0.04
            : 0.45 + random() * 0.45
          : random() * 0.04
        const unknownLines = Math.round(added * random() * 0.06)
        const aiLines = Math.round((added - unknownLines) * aiShare)
        const humanLines = added - unknownLines - aiLines

        const sessionIds: string[] = []
        let model: string | null = null
        let requests = 0
        let tokens = 0
        let costUsd10000 = 0
        if (tool) {
          const sessionKey = `${repository.name}:${tool}`
          let session = daySessions.get(sessionKey)
          if (!session || random() < 0.3) {
            sessionSeq += 1
            const sessionModel = pick(random, TOOL_MODELS[tool])
            session = {
              id: `s_${hex(random, 14)}`,
              userId: user.id,
              repository: repository.name,
              tool,
              model: sessionModel,
              startedAt: at - Math.round(logNormal(random, 40, 0.6)) * 60_000,
              requests: 0,
              tokens: 0,
              costUsd10000: 0,
              steeringTurns: 0,
              generatedLines: 0,
              joined: random() < 0.9,
              commitShas: [],
            }
            daySessions.set(sessionKey, session)
            sessions.push(session)
          }
          model = session.model
          sessionIds.push(session.id)
          if (session.joined) {
            requests = Math.round(aiLines * (0.35 + random() * 0.5) * repository.thrash) + 6
            tokens = requests * (MODEL_REQUEST_TOKENS[session.model] ?? 30_000)
            costUsd10000 = requests * (MODEL_REQUEST_COST[session.model] ?? 200) * COST_SCALE
          }
          session.requests += requests
          session.tokens += tokens
          session.costUsd10000 += costUsd10000
          session.steeringTurns += Math.round(logNormal(random, 1.2 * repository.thrash, 0.7))
          session.generatedLines += Math.round(aiLines * (1.6 + random() * 2.4 * repository.thrash))
        }

        const branchKey = `${user.id}:${repository.name}`
        let branch = openBranches.get(branchKey)
        if (!branch) {
          branch = {
            repository: repository.name,
            userId: user.id,
            size: 1 + Math.floor(random() * 5),
            title: PR_TITLES[Math.floor(random() * PR_TITLES.length)],
            commits: [],
            direct: random() < 0.07,
          }
          openBranches.set(branchKey, branch)
        }

        const commit: MockCommit = {
          sha: hex(random, 40),
          repository: repository.name,
          userId: user.id,
          at,
          branch: branch.direct ? 'main' : 'pending',
          isDefaultBranch: branch.direct,
          aiLines,
          humanLines,
          unknownLines,
          sessionIds,
          tool,
          model,
          requests,
          tokens,
          costUsd10000,
          pullRequestNumber: null,
        }
        commits.push(commit)
        for (const id of sessionIds) {
          sessions.find((session) => session.id === id)?.commitShas.push(commit.sha)
        }
        branch.commits.push(commit)
        if (branch.commits.length >= branch.size) {
          closeBranch(branch)
          openBranches.delete(branchKey)
        }
      }

      // Sessions that never reach a commit: exploration, abandoned attempts, reviews.
      const idleSessions = Math.floor(user.aiAffinity * weekdayFactor * ramp * 1.3 + random())
      for (let index = 0; index < idleSessions; index += 1) {
        const tool = pick(random, Object.entries(user.tools) as [AgentTool, number][])
        const model = pick(random, TOOL_MODELS[tool])
        const repository = pick(
          random,
          MOCK_REPOSITORIES.map((repo) => [repo.name, repo.weights[user.team] ?? 0.05]),
        )
        const requests = Math.round(logNormal(random, 18, 0.9))
        sessionSeq += 1
        sessions.push({
          id: `s_${hex(random, 14)}`,
          userId: user.id,
          repository,
          tool,
          model,
          startedAt: dayStart + (user.peakHour + (random() - 0.5) * 8) * HOUR_MS,
          requests,
          tokens: requests * (MODEL_REQUEST_TOKENS[model] ?? 30_000),
          costUsd10000: requests * (MODEL_REQUEST_COST[model] ?? 200) * COST_SCALE,
          steeringTurns: Math.round(logNormal(random, 3, 0.6)),
          generatedLines: Math.round(logNormal(random, 90, 1)),
          joined: true,
          commitShas: [],
        })
      }
    }
  }

  // Work still in flight at "now" stays on its branch, with no pull request yet.
  for (const branch of openBranches.values()) {
    for (const commit of branch.commits) {
      if (commit.branch === 'pending') commit.branch = `${branch.userId.slice(2)}/wip`
    }
  }

  return { now: MOCK_NOW, commits, sessions, pullRequests }
}

let cached: MockDataset | null = null

/** The whole generated history. Built once, then shared. */
export function getMockDataset(): MockDataset {
  cached ??= generate()
  return cached
}

export function mockUser(id: string) {
  return MOCK_USERS.find((user) => user.id === id) ?? MOCK_USERS[0]
}

/** Short repository name for dense labels: `acme/gateway`. */
export function shortRepository(name: string) {
  return name.replace(/^[^/]+\//, '')
}
