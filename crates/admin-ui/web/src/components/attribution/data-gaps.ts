// The attribution data Oceans does not collect yet, with how to collect each piece. The full
// collection plan lives in docs/internal/2026-11-15-attribution-data-gaps.md.

import type { Readiness } from './format'

export const DATA_GAPS_URL =
  'https://github.com/ahstn/oceans-llm/blob/main/docs/internal/2026-11-15-attribution-data-gaps.md'

/** What each piece of data to collect unlocks, smallest first. The live Durability tab and its
 * concept mockup both read this, so their readiness stories cannot drift apart. */
export const DATA_GAPS: {
  data: string
  unlocks: string
  avenue: string
  effort: 'Small' | 'Medium' | 'Large'
  today: Readiness
}[] = [
  {
    data: 'Pull request opened, reviews, open and closed pull requests',
    unlocks: 'Review wait, merged without review, abandon rate, in-review stage',
    avenue: 'More fields from the gh call oceans hook prs already makes, or a GitHub App',
    effort: 'Small',
    today: 'new',
  },
  {
    data: 'User turns per agent session',
    unlocks: 'Steering and autonomy per harness and model',
    avenue: 'Count user messages in requests the gateway already logs',
    effort: 'Small',
    today: 'new',
  },
  {
    data: 'Revert and fix-forward links',
    unlocks: 'Change fail rate by AI share',
    avenue: 'Revert commit bodies in the PR hook; titles are matched today',
    effort: 'Small',
    today: 'proxy',
  },
  {
    data: 'Deleted lines and files changed per commit',
    unlocks: 'Refactor rate, true batch size, net change',
    avenue: 'git diff --numstat totals at session end, no paths',
    effort: 'Medium',
    today: 'new',
  },
  {
    data: 'Generated lines per session',
    unlocks: 'Generated : committed, discarded work',
    avenue: 'git-ai checkpoint stats sent by the session-end hook',
    effort: 'Medium',
    today: 'new',
  },
  {
    data: 'Deploy and incident events',
    unlocks: 'Real DORA deploy frequency, lead time, failure and recovery',
    avenue: 'GitHub Deployments, CD webhooks (Argo CD, Flux), incident tools',
    effort: 'Large',
    today: 'new',
  },
  {
    data: 'Line survival at 14, 30 and 90 days',
    unlocks: 'Durability, churn, cost per durable line, readiness quadrant',
    avenue: 'A scheduled git ai blame pass where the clone lives, sending counts only',
    effort: 'Large',
    today: 'new',
  },
]
