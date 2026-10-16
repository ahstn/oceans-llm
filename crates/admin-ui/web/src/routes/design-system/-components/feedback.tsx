import { HugeiconsIcon } from '@hugeicons/react'
import {
  Alert02Icon,
  CheckmarkCircle02Icon,
  InboxIcon,
  InformationCircleIcon,
  Key01Icon,
} from '@hugeicons/core-free-icons'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty'
import { Progress } from '@/components/ui/progress'
import { Skeleton } from '@/components/ui/skeleton'
import { Spinner } from '@/components/ui/spinner'

import { ShowcaseBlock, ShowcaseRow, ShowcaseSection } from './showcase'

export function FeedbackSection() {
  return (
    <ShowcaseSection
      id="feedback"
      title="Feedback"
      description="Alerts for page-level state, toasts for confirmations, Progress and Skeleton while work is pending."
    >
      <div className="grid gap-4 lg:grid-cols-2">
        <ShowcaseBlock
          title="Alert"
          description="Default for notices, destructive for blocking errors."
        >
          <Alert>
            <HugeiconsIcon icon={InformationCircleIcon} />
            <AlertTitle>Budgets reset on Monday</AlertTitle>
            <AlertDescription>Weekly limits roll over at 00:00 UTC.</AlertDescription>
          </Alert>
          <Alert variant="destructive">
            <HugeiconsIcon icon={Alert02Icon} />
            <AlertTitle>Provider unreachable</AlertTitle>
            <AlertDescription>
              Requests to this model fail until the key is rotated.
            </AlertDescription>
          </Alert>
        </ShowcaseBlock>

        <ShowcaseBlock
          title="Toast"
          description="sonner, bottom right. Confirm what happened; offer undo where possible."
        >
          <ShowcaseRow label="Trigger">
            <Button
              variant="outline"
              onClick={() => toast.success('API key created', { description: 'ci-pipeline' })}
            >
              <HugeiconsIcon icon={CheckmarkCircle02Icon} data-icon="inline-start" />
              Success
            </Button>
            <Button
              variant="outline"
              onClick={() => toast.error('Could not revoke key', { description: 'Try again.' })}
            >
              <HugeiconsIcon icon={Alert02Icon} data-icon="inline-start" />
              Error
            </Button>
            <Button
              variant="outline"
              onClick={() =>
                toast('Key revoked', { action: { label: 'Undo', onClick: () => undefined } })
              }
            >
              With action
            </Button>
          </ShowcaseRow>
        </ShowcaseBlock>

        <ShowcaseBlock
          title="Progress and Spinner"
          description="Determinate first; Spinner when the length is unknown."
        >
          <div className="flex flex-col gap-2">
            <div className="flex items-baseline justify-between text-sm">
              <span>Weekly budget</span>
              <span className="text-muted-foreground tabular-nums">$186.40 of $250.00</span>
            </div>
            <Progress value={74.5} aria-label="Weekly budget used" />
          </div>
          <ShowcaseRow label="Spinner">
            <Spinner />
            <span className="text-muted-foreground text-sm">Loading request logs…</span>
          </ShowcaseRow>
        </ShowcaseBlock>

        <ShowcaseBlock title="Skeleton" description="Match the shape of what is loading.">
          <div className="flex items-center gap-3">
            <Skeleton className="size-10 rounded-full" />
            <div className="flex flex-1 flex-col gap-2">
              <Skeleton className="h-4 w-1/2" />
              <Skeleton className="h-3 w-1/3" />
            </div>
          </div>
          <Skeleton className="h-24 w-full" />
        </ShowcaseBlock>
      </div>

      <ShowcaseBlock
        title="Empty"
        description="Say what's missing and offer the one action that fixes it."
      >
        <Empty className="border">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <HugeiconsIcon icon={InboxIcon} />
            </EmptyMedia>
            <EmptyTitle>No API keys yet</EmptyTitle>
            <EmptyDescription>Create a key to call models through the gateway.</EmptyDescription>
          </EmptyHeader>
          <EmptyContent>
            <Button>
              <HugeiconsIcon icon={Key01Icon} data-icon="inline-start" />
              Create API key
            </Button>
          </EmptyContent>
        </Empty>
      </ShowcaseBlock>
    </ShowcaseSection>
  )
}
