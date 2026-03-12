import type { ReactNode } from 'react'

import { Card, CardContent, CardDescription, CardHeader } from '@/components/ui/card'
import { cn } from '@/lib/utils'

interface AuthLayoutProps {
  title: string
  description: ReactNode
  eyebrow?: string
  children: ReactNode
  cardClassName?: string
}

export function AuthLayout({
  title,
  description,
  eyebrow = 'Oceans Gateway',
  children,
  cardClassName,
}: AuthLayoutProps) {
  return (
    <div className="min-h-screen overflow-hidden px-4 py-6 text-[var(--color-text)] sm:px-8 sm:py-10">
      <div className="mx-auto grid min-h-[calc(100vh-3rem)] max-w-6xl items-center gap-8 lg:grid-cols-[1.05fr_0.95fr]">
        <div className="hidden flex-col gap-5 lg:flex">
          <div className="flex flex-col gap-3">
            <span className="text-xs font-semibold tracking-[0.18em] text-[var(--color-primary)] uppercase">
              {eyebrow}
            </span>
            <h1 className="max-w-xl text-[clamp(2.8rem,5vw,4.6rem)] leading-[0.94] font-[var(--font-display)] text-[var(--color-text)]">
              Secure the control plane without losing the calm.
            </h1>
            <p className="max-w-lg text-base text-[var(--color-text-muted)]">
              Identity onboarding, bootstrap access, and admin handoffs should feel explicit,
              reliable, and easy to scan on the first pass.
            </p>
          </div>

          <div className="grid max-w-xl gap-4 md:grid-cols-2">
            {[
              [
                'Session-first flows',
                'Password rotation and invite completion stay server-backed.',
              ],
              [
                'Operational clarity',
                'Every screen should communicate the next safe action immediately.',
              ],
            ].map(([label, copy]) => (
              <div key={label} className="rounded-lg border border-[color:var(--color-border)] p-4">
                <p className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                  {label}
                </p>
                <p className="mt-2 text-sm text-[var(--color-text-muted)]">{copy}</p>
              </div>
            ))}
          </div>
        </div>

        <div>
          <Card
            className={cn(
              'w-full border-[color:var(--color-border-strong)] bg-[var(--color-card-auth)]',
              cardClassName,
            )}
          >
            <CardHeader className="gap-4">
              <div className="flex items-center justify-between gap-4">
                <span className="text-xs font-semibold tracking-[0.18em] text-[var(--color-primary)] uppercase">
                  Admin access
                </span>
                <span className="rounded-full border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] px-3 py-1 text-[11px] font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                  Control plane
                </span>
              </div>
              <div className="flex flex-col gap-3">
                <h2 className="text-[clamp(1.9rem,2vw,2.6rem)] leading-tight font-[var(--font-display)] text-[var(--color-text)]">
                  {title}
                </h2>
                <CardDescription className="max-w-xl">{description}</CardDescription>
              </div>
            </CardHeader>
            <CardContent className="flex flex-col gap-5">{children}</CardContent>
          </Card>
        </div>
      </div>
    </div>
  )
}
