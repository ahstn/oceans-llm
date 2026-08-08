import type { ReactNode } from 'react'

export function PageHeader({
  section,
  title,
  description,
  actions,
}: {
  section: string
  title: string
  description: ReactNode
  actions?: ReactNode
}) {
  return (
    <header className="flex flex-col gap-2">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <p className="text-muted-foreground text-sm">{section}</p>
          <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
        </div>
        {actions}
      </div>
      <p className="text-muted-foreground max-w-3xl text-sm">{description}</p>
    </header>
  )
}
