import { createContext, useCallback, useContext, useEffect, useId, useMemo, useState } from 'react'
import type { ComponentProps, CSSProperties, ReactNode } from 'react'

import {
  codeBlockTokenStyle,
  highlightCode,
  toPlainLines,
} from '@/components/reui/code-block/code-block-highlight'
import type {
  CodeBlockLine,
  CodeBlockToken,
} from '@/components/reui/code-block/code-block-highlight'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

type CodeBlockContextValue = {
  code: string
}

const CodeBlockContext = createContext<CodeBlockContextValue | null>(null)

function useCodeBlock() {
  const context = useContext(CodeBlockContext)
  if (!context) throw new Error('Code block controls must be rendered inside CodeBlock')
  return context
}

type CodeBlockProps = Omit<ComponentProps<'div'>, 'children'> & {
  code: string
  language?: string
  showLineNumbers?: boolean
  maxLines?: number
  children?: ReactNode
}

type HighlightResult = {
  code: string
  language?: string
  lines: CodeBlockLine[]
}

function CodeBlock({
  code,
  language,
  showLineNumbers = false,
  maxLines,
  children,
  className,
  ...props
}: CodeBlockProps) {
  const [highlighted, setHighlighted] = useState<HighlightResult | null>(null)
  const fallbackLines = useMemo(() => toPlainLines(code), [code])
  const contentId = useId()

  useEffect(() => {
    let cancelled = false
    void highlightCode(code, language).then((lines) => {
      if (!cancelled) setHighlighted({ code, language, lines })
    })
    return () => {
      cancelled = true
    }
  }, [code, language])

  const lines =
    highlighted?.code === code && highlighted.language === language
      ? highlighted.lines
      : fallbackLines
  const context = useMemo(() => ({ code }), [code])
  const normalizedMaxLines = maxLines ? Math.max(1, Math.floor(maxLines)) : undefined
  const viewportStyle = normalizedMaxLines
    ? ({
        maxHeight: `calc(${normalizedMaxLines} * 1.5rem + 2rem)`,
      } satisfies CSSProperties)
    : undefined

  return (
    <CodeBlockContext.Provider value={context}>
      <div
        data-slot="code-block"
        className={cn(
          'bg-muted/30 border-border flex max-w-full min-w-0 flex-col overflow-hidden rounded-lg border font-mono text-sm',
          className,
        )}
        {...props}
      >
        {children}
        <div
          id={contentId}
          data-slot="code-block-viewport"
          data-vertical-scroll={normalizedMaxLines ? true : undefined}
          role="region"
          aria-label={language ? `${language} code` : 'Code'}
          tabIndex={0}
          className="focus-visible:ring-ring max-w-full min-w-0 overflow-auto overscroll-contain py-4 outline-none focus-visible:ring-2 focus-visible:ring-inset"
          style={viewportStyle}
        >
          <pre className="min-w-max text-[13px] leading-6" dir="ltr">
            <code>
              {lines.map((line) => (
                <CodeBlockLineRow key={line.number} line={line} showLineNumber={showLineNumbers} />
              ))}
            </code>
          </pre>
        </div>
      </div>
    </CodeBlockContext.Provider>
  )
}

type CodeBlockLineRowProps = {
  line: CodeBlockLine
  showLineNumber: boolean
}

function CodeBlockLineRow({ line, showLineNumber }: CodeBlockLineRowProps) {
  let tokenOffset = 0

  return (
    <span
      data-slot="code-block-line"
      className={cn(
        'grid min-h-6 w-full grid-cols-[1fr] px-4',
        showLineNumber && 'grid-cols-[auto_1fr] px-0',
      )}
    >
      {showLineNumber ? (
        <span
          aria-hidden="true"
          className="text-muted-foreground/70 bg-muted/70 sticky left-0 z-10 w-12 border-r px-3 text-right select-none"
        >
          {line.number}
        </span>
      ) : null}
      <span className={cn('whitespace-pre', showLineNumber && 'px-4')}>
        {line.tokens.length > 0
          ? line.tokens.map((token) => {
              const start = tokenOffset
              tokenOffset += token.content.length
              return <CodeBlockTokenSpan key={`${line.number}:${start}`} token={token} />
            })
          : '\u200b'}
      </span>
    </span>
  )
}

type CodeBlockTokenSpanProps = {
  token: CodeBlockToken
}

function CodeBlockTokenSpan({ token }: CodeBlockTokenSpanProps) {
  return (
    <span
      className="text-(--code-token-light) dark:text-(--code-token-dark)"
      style={codeBlockTokenStyle(token)}
    >
      {token.content}
    </span>
  )
}

type CodeBlockHeaderProps = ComponentProps<'div'>

function CodeBlockHeader({ className, ...props }: CodeBlockHeaderProps) {
  return (
    <div
      data-slot="code-block-header"
      className={cn(
        'border-border bg-muted/60 flex min-h-10 min-w-0 items-center gap-2 border-b px-3 py-1.5 font-sans',
        className,
      )}
      {...props}
    />
  )
}

type CodeBlockTitleProps = ComponentProps<'div'>

function CodeBlockTitle({ className, ...props }: CodeBlockTitleProps) {
  return (
    <div
      data-slot="code-block-title"
      className={cn('text-foreground min-w-0 truncate text-xs font-medium', className)}
      {...props}
    />
  )
}

type CopyLabels = {
  copy?: string
  copied?: string
  failed?: string
}

type CodeBlockCopyButtonProps = Omit<ComponentProps<typeof Button>, 'onError'> & {
  value?: string
  timeout?: number
  labels?: CopyLabels
  onCopy?: (value: string) => void
  onCopyError?: (error: unknown) => void
}

type CopyState = 'idle' | 'copied' | 'failed'

function CodeBlockCopyButton({
  value,
  timeout = 2000,
  labels,
  onCopy,
  onCopyError,
  className,
  children,
  variant = 'ghost',
  size = 'icon-sm',
  ...props
}: CodeBlockCopyButtonProps) {
  const { code } = useCodeBlock()
  const [state, setState] = useState<CopyState>('idle')

  useEffect(() => {
    if (state === 'idle' || timeout === 0) return
    const timer = window.setTimeout(() => setState('idle'), timeout)
    return () => window.clearTimeout(timer)
  }, [state, timeout])

  const copy = useCallback(async () => {
    const payload = value ?? code
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error('Clipboard API is unavailable')
      }
      await navigator.clipboard.writeText(payload)
      setState('copied')
      onCopy?.(payload)
    } catch (error) {
      setState('failed')
      onCopyError?.(error)
    }
  }, [code, onCopy, onCopyError, value])

  const label =
    state === 'copied'
      ? (labels?.copied ?? 'Copied')
      : state === 'failed'
        ? (labels?.failed ?? 'Copy failed')
        : (labels?.copy ?? 'Copy code')

  return (
    <Button
      type="button"
      data-slot="code-block-copy"
      data-copy-state={state}
      aria-label={label}
      variant={variant}
      size={size}
      className={cn('shrink-0', className)}
      onClick={() => void copy()}
      {...props}
    >
      {children ?? (state === 'copied' ? <CheckIcon /> : <CopyIcon />)}
    </Button>
  )
}

function CopyIcon() {
  return (
    <svg data-icon="inline-start" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M8 8V5.6A1.6 1.6 0 0 1 9.6 4h8.8A1.6 1.6 0 0 1 20 5.6v8.8a1.6 1.6 0 0 1-1.6 1.6H16"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <rect x="4" y="8" width="12" height="12" rx="1.6" stroke="currentColor" strokeWidth="1.75" />
    </svg>
  )
}

function CheckIcon() {
  return (
    <svg data-icon="inline-start" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="m5 12.5 4.2 4.2L19 7"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}

export { CodeBlock, CodeBlockCopyButton, CodeBlockHeader, CodeBlockTitle }
export type { CodeBlockCopyButtonProps, CodeBlockHeaderProps, CodeBlockProps, CodeBlockTitleProps }
