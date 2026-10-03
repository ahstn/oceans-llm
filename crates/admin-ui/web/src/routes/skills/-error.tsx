import { useTransition } from 'react'
import { useRouter, type ErrorComponentProps } from '@tanstack/react-router'

import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Spinner } from '@/components/ui/spinner'
import { getErrorMessage } from '@/lib/errors'

export function SkillsErrorPage({ error }: ErrorComponentProps) {
  const router = useRouter()
  const [pending, startTransition] = useTransition()

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
