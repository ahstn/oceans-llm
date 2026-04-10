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
    <div className="bg-background text-foreground min-h-screen overflow-hidden px-4 py-6 sm:px-8 sm:py-10">
      <div className="mx-auto grid min-h-[calc(100vh-3rem)] max-w-6xl items-center gap-8 lg:grid-cols-[1.05fr_0.95fr]">
        <div className="hidden flex-col gap-5 lg:flex">
          <div className="flex flex-col gap-3">
            <span className="text-primary text-xs font-semibold tracking-[0.18em] uppercase">
              {eyebrow}
            </span>
            <h1 className="font-heading max-w-xl text-[clamp(2.8rem,5vw,4.6rem)] leading-[0.94]">
              Secure the control plane without losing the calm.
            </h1>
            <p className="text-muted-foreground max-w-lg text-base">
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
              <div key={label} className="bg-card rounded-xl border p-4 shadow-sm">
                <p className="text-muted-foreground text-xs font-semibold tracking-[0.08em] uppercase">
                  {label}
                </p>
                <p className="text-muted-foreground mt-2 text-sm">{copy}</p>
              </div>
            ))}
          </div>
        </div>

        <div>
          <Card
            className={cn(
              'border-border/80 bg-card/95 w-full shadow-xl backdrop-blur',
              cardClassName,
            )}
          >
            <CardHeader className="gap-4">
              <div className="flex items-center justify-between gap-4">
                <span className="text-primary text-xs font-semibold tracking-[0.18em] uppercase">
                  Admin access
                </span>
                <span className="bg-muted text-muted-foreground rounded-full border px-3 py-1 text-[11px] font-semibold tracking-[0.08em] uppercase">
                  Control plane
                </span>
              </div>
              <div className="flex flex-col gap-3">
                <h2 className="font-heading text-[clamp(1.9rem,2vw,2.6rem)] leading-tight">
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
