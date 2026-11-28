import { useTransition } from 'react'
import { useRouter, type ErrorComponentProps } from '@tanstack/react-router'
import { LinkSquare02Icon, PackageIcon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty'
import { Spinner } from '@/components/ui/spinner'
import { getErrorMessage } from '@/lib/errors'

const SKILLS_DOCS_URL = 'https://oceans-llm.com/extensions/skills/'

const SKILLS_CONFIG = `skills:
  enabled: true
  storage:
    bucket: oceans-skills`

// The gateway answers every skills call with this message until `skills` is configured.
function isSkillsNotConfigured(error: unknown) {
  return /skills are not configured/i.test(getErrorMessage(error, ''))
}

export function SkillsErrorPage({ error }: ErrorComponentProps) {
  const router = useRouter()
  const [pending, startTransition] = useTransition()

  if (isSkillsNotConfigured(error)) {
    return <SkillsSetupEmpty />
  }

  return (
    <Card className="min-w-0">
      <CardHeader>
        <CardTitle>Skills are unavailable</CardTitle>
        <CardDescription>
          This page could not load. Other sections of Oceans are still available.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex min-w-0 flex-col items-start gap-4">
        <Alert variant="destructive">
          <AlertDescription className="break-words">
            {getErrorMessage(error, 'Unable to load skills. Please try again.')}
          </AlertDescription>
        </Alert>
        <Button
          disabled={pending}
          onClick={() =>
            startTransition(async () => {
              await router.invalidate({ sync: true })
            })
          }
        >
          {pending ? <Spinner data-icon="inline-start" /> : null}
          Try again
        </Button>
      </CardContent>
    </Card>
  )
}

/** Skills storage is off: say what turns it on rather than offering a retry that can't work. */
function SkillsSetupEmpty() {
  return (
    <Empty className="bg-surface-muted rounded-xl border">
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <AppIcon icon={PackageIcon} size={22} stroke={1.5} />
        </EmptyMedia>
        <EmptyTitle>Skills storage is off</EmptyTitle>
        <EmptyDescription>
          The gateway keeps each skill version in an S3 bucket. Create a private bucket, add a{' '}
          <code className="font-mono">skills</code> section to the gateway config, then restart the
          gateway.
        </EmptyDescription>
      </EmptyHeader>
      <EmptyContent>
        <pre className="bg-background text-foreground w-full overflow-x-auto rounded px-3 py-2 text-left font-mono text-xs">
          {SKILLS_CONFIG}
        </pre>
        <Button variant="outline" size="sm" asChild>
          <a href={SKILLS_DOCS_URL} target="_blank" rel="noopener noreferrer">
            Set up skills storage
            <AppIcon icon={LinkSquare02Icon} data-icon="inline-end" aria-hidden />
          </a>
        </Button>
      </EmptyContent>
    </Empty>
  )
}
