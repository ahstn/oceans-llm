import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import Markdown from 'react-markdown'

import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { getSkillFile } from '@/server/skills-data.functions'
import type { SkillVersionDetail } from '@/types/skills-api'

export function SkillFiles({ id, detail }: { id: string; detail: SkillVersionDetail }) {
  const [path, setPath] = useState('SKILL.md')
  const preview = useQuery({
    queryKey: ['skill-file', id, detail.version.version, path],
    queryFn: () => getSkillFile({ data: { id, version: detail.version.version, path } }),
    enabled: path !== 'SKILL.md',
    retry: false,
  })

  return (
    <Card className="min-w-0 overflow-hidden">
      <CardHeader>
        <CardTitle>Instructions and files</CardTitle>
        <CardDescription>Browse the version contents. File previews are read-only.</CardDescription>
      </CardHeader>
      <CardContent className="grid min-w-0 gap-6 md:grid-cols-[15rem_minmax(0,1fr)]">
        <nav aria-label="Skill files" className="min-w-0 overflow-hidden">
          <ul className="flex max-h-96 flex-col gap-1 overflow-y-auto">
            {detail.files.map((file) => (
              <li key={file.path} className="min-w-0">
                <Button
                  variant={path === file.path ? 'secondary' : 'ghost'}
                  size="sm"
                  className="w-full justify-start"
                  onClick={() => setPath(file.path)}
                  aria-current={path === file.path ? 'true' : undefined}
                  title={`${file.path} (${file.size.toLocaleString()} bytes)`}
                >
                  <span className="truncate font-mono">{file.path}</span>
                </Button>
              </li>
            ))}
          </ul>
        </nav>
        <section
          aria-label={`Preview of ${path}`}
          className="flex max-w-full min-w-0 flex-col gap-4 overflow-hidden"
        >
          <h2 className="truncate font-mono text-sm" title={path}>
            {path}
          </h2>
          {path === 'SKILL.md' ? (
            <SkillInstructions source={detail.instructions} />
          ) : preview.isPending ? (
            <Skeleton className="h-32 w-full" />
          ) : preview.error ? (
            <Alert variant="destructive">
              <AlertDescription>
                {preview.error instanceof Error
                  ? preview.error.message
                  : 'This file cannot be previewed.'}
              </AlertDescription>
            </Alert>
          ) : (
            <pre
              data-testid="skill-file-text"
              className="bg-muted max-w-full min-w-0 overflow-x-auto rounded-md p-4 text-sm"
            >
              <code>{preview.data.content}</code>
            </pre>
          )}
        </section>
      </CardContent>
    </Card>
  )
}

export function SkillInstructions({ source }: { source: string }) {
  const instructions = source.replace(/^---\r?\n[\s\S]*?\r?\n---(?:\r?\n|$)/, '')
  return (
    <div
      data-testid="skill-instructions"
      className="[&_pre]:bg-muted flex min-w-0 flex-col gap-3 text-sm leading-relaxed break-words [&_blockquote]:border-l-2 [&_blockquote]:pl-4 [&_code]:font-mono [&_h1]:text-xl [&_h1]:font-semibold [&_h2]:text-lg [&_h2]:font-semibold [&_h3]:font-medium [&_li]:ml-5 [&_ol]:list-decimal [&_pre]:max-w-full [&_pre]:overflow-x-auto [&_pre]:rounded-md [&_pre]:p-4 [&_ul]:list-disc"
    >
      <Markdown
        skipHtml
        components={{
          img: ({ alt }) => (
            <span className="text-muted-foreground">[Image: {alt || 'omitted'}]</span>
          ),
          a: ({ href, children }) =>
            href && /^https?:\/\//i.test(href) ? (
              <a
                href={href}
                target="_blank"
                rel="noopener noreferrer"
                className="underline underline-offset-4"
              >
                {children}
              </a>
            ) : (
              <span>{children}</span>
            ),
        }}
      >
        {instructions}
      </Markdown>
    </div>
  )
}
