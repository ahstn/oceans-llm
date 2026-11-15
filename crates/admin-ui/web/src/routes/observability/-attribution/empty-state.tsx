import { GitCommitIcon, LinkSquare02Icon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { Button } from '@/components/ui/button'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty'

import { ATTRIBUTION_DOCS_URL } from './shared'

const INSTALL_COMMANDS = [
  'oceans hook install --git-ai --claude',
  'oceans hook install --git-ai --omp',
] as const

/** No attribution data at all: explain how commits start reaching Oceans. */
export function AttributionSetupEmpty({ windowDays }: { windowDays: number }) {
  return (
    <Empty className="bg-surface-muted rounded-xl border">
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <AppIcon icon={GitCommitIcon} size={22} stroke={1.5} />
        </EmptyMedia>
        <EmptyTitle>No attributed commits yet</EmptyTitle>
        <EmptyDescription>
          Commits from the last {windowDays} days appear here once Git AI notes reach Oceans. On
          each machine that commits agent work, sign in with{' '}
          <code className="font-mono">oceans login</code>, then install the hooks for Claude Code or
          Oh My Pi.
        </EmptyDescription>
      </EmptyHeader>
      <EmptyContent>
        <div className="flex w-full flex-col gap-2">
          {INSTALL_COMMANDS.map((command) => (
            <code
              key={command}
              className="bg-background text-foreground block overflow-x-auto rounded px-3 py-2 text-left text-xs"
            >
              {command}
            </code>
          ))}
        </div>
        <Button variant="outline" size="sm" asChild>
          <a href={ATTRIBUTION_DOCS_URL} target="_blank" rel="noopener noreferrer">
            Set up Git AI attribution
            <AppIcon icon={LinkSquare02Icon} data-icon="inline-end" aria-hidden />
          </a>
        </Button>
      </EmptyContent>
    </Empty>
  )
}

/** Nothing in the window matches the active user or repository filter. */
export function AttributionFilteredEmpty({ onClear }: { onClear: () => void }) {
  return (
    <Empty className="bg-surface-muted rounded-xl border">
      <EmptyHeader>
        <EmptyTitle>No attribution matches these filters</EmptyTitle>
        <EmptyDescription>
          Clear the user or repository filter, or choose a longer window.
        </EmptyDescription>
      </EmptyHeader>
      <EmptyContent>
        <Button type="button" variant="outline" size="sm" onClick={onClear}>
          Clear filters
        </Button>
      </EmptyContent>
    </Empty>
  )
}
