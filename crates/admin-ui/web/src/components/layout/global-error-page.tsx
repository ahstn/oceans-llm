import type { ErrorComponentProps } from '@tanstack/react-router'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Separator } from '@/components/ui/separator'

const DEFAULT_ERROR_TITLE = 'The admin UI could not load'
const DEFAULT_ERROR_MESSAGE = 'An unexpected error stopped the admin control plane from rendering.'

export function getErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message
  }

  if (typeof error === 'string' && error.trim()) {
    return error
  }

  return DEFAULT_ERROR_MESSAGE
}

function getErrorName(error: unknown): string {
  if (error instanceof Error && error.name.trim()) {
    return error.name
  }

  return 'Error'
}

function buildDiagnosticDetails(error: unknown, componentStack?: string): string {
  const details = [`${getErrorName(error)}: ${getErrorMessage(error)}`]

  if (error instanceof Error && error.stack) {
    details.push('', error.stack)
  }

  if (componentStack) {
    details.push('', 'Component stack:', componentStack)
  }

  return details.join('\n')
}

async function copyTextToClipboard(text: string) {
  if (typeof navigator === 'undefined' || !navigator.clipboard) {
    return
  }

  await navigator.clipboard.writeText(text)
}

const diagnosticChecks = [
  ['Gateway origin', 'ADMIN_GATEWAY_ORIGIN'],
  ['Health check', '/healthz'],
  ['Container DNS', 'gateway:8080'],
] as const

export function GlobalErrorPage({ error, info, reset }: ErrorComponentProps) {
  const message = getErrorMessage(error)
  const details = buildDiagnosticDetails(error, info?.componentStack)

  return (
    <main className="bg-background text-foreground min-h-screen overflow-hidden px-4 py-6 sm:px-8 sm:py-10">
      <div className="mx-auto grid min-h-[calc(100vh-3rem)] max-w-6xl items-center gap-8 lg:grid-cols-[1.05fr_0.95fr]">
        <div className="hidden flex-col gap-5 lg:flex">
          <div className="flex flex-col gap-3">
            <Badge variant="ghost" className="text-primary w-fit px-0 tracking-[0.18em] uppercase">
              Oceans Gateway
            </Badge>
            <h1 className="font-heading max-w-xl text-[clamp(2.8rem,5vw,4.6rem)] leading-[0.94]">
              A clear stop when the control plane cannot continue.
            </h1>
            <p className="text-muted-foreground max-w-lg text-base">
              Runtime failures should expose the next useful check without hiding the original
              diagnostic message.
            </p>
          </div>

          <div className="grid max-w-xl gap-4 md:grid-cols-2">
            {[
              ['Original message', 'Keep the thrown error visible for fast triage.'],
              ['Operational hints', 'Point local and deploy users toward gateway connectivity.'],
            ].map(([label, copy]) => (
              <Card key={label} className="bg-card shadow-sm" size="sm">
                <CardContent className="flex flex-col gap-2">
                  <p className="text-muted-foreground text-xs font-semibold tracking-[0.08em] uppercase">
                    {label}
                  </p>
                  <p className="text-muted-foreground text-sm">{copy}</p>
                </CardContent>
              </Card>
            ))}
          </div>
        </div>

        <Card className="border-border/80 bg-card/95 w-full shadow-xl backdrop-blur">
          <CardHeader className="gap-4">
            <div className="flex items-center justify-between gap-4">
              <Badge variant="ghost" className="text-primary px-0 tracking-[0.18em] uppercase">
                Admin UI error
              </Badge>
              <Badge variant="destructive">{getErrorName(error)}</Badge>
            </div>
            <div className="flex flex-col gap-3">
              <CardTitle className="font-heading text-[clamp(1.9rem,2vw,2.6rem)] leading-tight">
                {DEFAULT_ERROR_TITLE}
              </CardTitle>
              <CardDescription className="max-w-xl text-base leading-7">{message}</CardDescription>
            </div>
          </CardHeader>

          <CardContent className="flex flex-col gap-5">
            <Alert variant="destructive">
              <AlertTitle>What to check</AlertTitle>
              <AlertDescription>
                This usually means a server-side loader or route guard failed before the page could
                render. If you are running Docker Compose, confirm the gateway is healthy and the
                admin UI has the correct gateway origin configured.
              </AlertDescription>
            </Alert>

            <div className="grid gap-3 text-sm sm:grid-cols-3">
              {diagnosticChecks.map(([label, value]) => (
                <Card key={label} className="bg-muted/30" size="sm">
                  <CardContent className="flex flex-col gap-1">
                    <p className="text-muted-foreground text-xs font-semibold tracking-[0.12em] uppercase">
                      {label}
                    </p>
                    <code className="font-mono text-xs break-all">{value}</code>
                  </CardContent>
                </Card>
              ))}
            </div>

            <Separator />

            <Card className="bg-muted/30" size="sm">
              <CardHeader className="flex-row items-center justify-between gap-3">
                <CardTitle>Diagnostic details</CardTitle>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => void copyTextToClipboard(details)}
                >
                  Copy details
                </Button>
              </CardHeader>
              <CardContent>
                <pre className="text-muted-foreground max-h-72 overflow-auto whitespace-pre-wrap rounded-lg font-mono text-xs leading-5">
                  {details}
                </pre>
              </CardContent>
            </Card>

            <div className="flex flex-col-reverse gap-3 sm:flex-row sm:justify-end">
              <Button type="button" variant="outline" onClick={() => window.location.reload()}>
                Reload page
              </Button>
              <Button type="button" onClick={reset}>
                Try again
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>
    </main>
  )
}
