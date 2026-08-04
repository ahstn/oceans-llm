import type { ReactNode } from 'react'

import authHeroWave from '@/assets/auth-hero-wave.png'
import oceansLogo from '@/assets/oceans-logo-rounded-square.png'
import { cn } from '@/lib/utils'

interface AuthLayoutProps {
  title: string
  description: ReactNode
  children: ReactNode
  cardClassName?: string
}

export function AuthLayout({ title, description, children, cardClassName }: AuthLayoutProps) {
  return (
    <main className="bg-background text-foreground min-h-screen px-4 py-4 sm:px-6 sm:py-6 lg:px-8 lg:py-8">
      <div className="mx-auto grid min-h-[calc(100vh-2rem)] max-w-7xl items-stretch gap-10 sm:min-h-[calc(100vh-3rem)] lg:grid-cols-[minmax(22rem,0.9fr)_minmax(28rem,1.1fr)] lg:gap-12 xl:grid-cols-[minmax(30rem,0.95fr)_minmax(32rem,1.05fr)] xl:gap-20">
        <section className="relative hidden min-h-[40rem] overflow-hidden rounded-xl border lg:flex">
          <img src={authHeroWave} alt="" className="absolute inset-0 size-full object-cover" />
          <div className="relative flex size-full flex-col justify-between p-10 xl:p-12">
            <div className="flex items-center gap-4">
              <img src={oceansLogo} alt="" className="size-14 rounded-xl" />
              <span className="font-heading text-xl font-medium">Oceans Gateway</span>
            </div>

            <p className="font-heading max-w-md text-3xl leading-tight font-medium text-balance">
              Secure the gateway.
              <br />
              Stay in control.
            </p>
          </div>
        </section>

        <section className="flex items-center justify-center py-8 lg:py-12">
          <div className={cn('flex w-full max-w-xl flex-col gap-8', cardClassName)}>
            <div className="flex items-center gap-3 lg:hidden">
              <img src={oceansLogo} alt="" className="size-10 rounded-lg" />
              <span className="font-heading text-lg font-medium">Oceans Gateway</span>
            </div>

            <header className="flex flex-col gap-3">
              <h1 className="font-heading text-4xl leading-tight font-medium text-balance sm:text-5xl">
                {title}
              </h1>
              <p className="text-muted-foreground max-w-lg text-base text-pretty">{description}</p>
            </header>

            <div className="flex flex-col gap-6">{children}</div>
          </div>
        </section>
      </div>
    </main>
  )
}
