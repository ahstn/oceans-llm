import { useLayoutEffect, useRef, useState, type ReactNode } from 'react'

export function PageHeader({
  section,
  title,
  description,
  actions,
  leading,
}: {
  section: string
  title: string
  description: ReactNode
  actions?: ReactNode
  /** Square element beside the whole header block, sized to its height (e.g. an avatar). */
  leading?: ReactNode
}) {
  const content = (
    <>
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <p className="text-muted-foreground text-sm">{section}</p>
          <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
        </div>
        {actions}
      </div>
      <p className="text-muted-foreground max-w-3xl text-sm">{description}</p>
    </>
  )
  if (!leading) return <header className="flex flex-col gap-2">{content}</header>
  return <LeadingHeader leading={leading}>{content}</LeadingHeader>
}

/**
 * CSS can stretch the leading element to the text height but cannot keep it square, so the
 * text block is measured. Before measurement (and on the server) it falls back to the
 * one-line-description height.
 */
function LeadingHeader({ leading, children }: { leading: ReactNode; children: ReactNode }) {
  const textRef = useRef<HTMLDivElement>(null)
  const [height, setHeight] = useState<number>()

  useLayoutEffect(() => {
    const text = textRef.current
    if (!text || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(([entry]) => setHeight(entry.borderBoxSize[0]?.blockSize))
    observer.observe(text)
    return () => observer.disconnect()
  }, [])

  return (
    <header className="flex items-start gap-4">
      <div
        className="size-20 shrink-0 [&>*]:size-full"
        style={height ? { width: height, height } : undefined}
      >
        {leading}
      </div>
      <div ref={textRef} className="flex min-w-0 flex-1 flex-col gap-2">
        {children}
      </div>
    </header>
  )
}
