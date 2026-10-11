import { useState } from 'react'
import { Search01Icon } from '@hugeicons/core-free-icons'
import { createFileRoute, Link, useRouter, useRouterState } from '@tanstack/react-router'
import { toast } from 'sonner'

import { PageHeader } from '@/components/layout/page-header'
import { AppIcon } from '@/components/icons/app-icon'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { getSkills } from '@/server/skills-data.functions'
import type { SkillDetail, SkillSummary } from '@/types/skills-api'
import { SkillUploadDialog } from './-upload-dialog'
import { SkillsErrorPage } from './-error'

export const Route = createFileRoute('/skills/')({
  validateSearch: (search: Record<string, unknown>): { offset: number; q?: string } => ({
    offset:
      typeof search.offset === 'number' && Number.isSafeInteger(search.offset) && search.offset > 0
        ? search.offset
        : 0,
    q: typeof search.q === 'string' ? search.q : undefined,
  }),
  loaderDeps: ({ search }) => search,
  loader: ({ deps }) => getSkills({ data: deps }),
  component: SkillsPage,
  errorComponent: SkillsErrorPage,
})

function SkillsPage() {
  const { items, namespace, limits } = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()
  const [uploadOpen, setUploadOpen] = useState(false)
  const query = useRouterState({
    select: (state) => (typeof state.location.search.q === 'string' ? state.location.search.q : ''),
  })
  const searchPending = query !== (search.q ?? '')

  async function uploaded(detail: SkillDetail) {
    setUploadOpen(false)
    toast.success('Skill uploaded')
    await router.invalidate()
    await router.navigate({ to: '/skills/$id', params: { id: detail.skill.id } })
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Agent Extensions"
        title="Agent Skills"
        description="Share re-usable agent skills. All users can fetch or add new skills."
        actions={<Button onClick={() => setUploadOpen(true)}>Upload skill</Button>}
      />
      <Card className="min-w-0">
        <CardHeader>
          <CardTitle>Skill catalog</CardTitle>
          <CardDescription>Filter, view and download existing skills.</CardDescription>
        </CardHeader>
        <CardContent className="flex min-w-0 flex-col gap-5">
          <InputGroup className="w-full sm:max-w-xs">
            <InputGroupInput
              aria-label="Search skills"
              placeholder="Search skills…"
              value={query}
              onChange={(event) => {
                const value = event.target.value
                void router.navigate({
                  to: '/skills',
                  search: { offset: 0, q: value || undefined },
                  replace: true,
                  resetScroll: false,
                })
              }}
            />
            <InputGroupAddon>
              <AppIcon icon={Search01Icon} aria-hidden />
            </InputGroupAddon>
          </InputGroup>
          <SkillCatalog items={items} />
          <div className="flex items-center justify-between gap-3">
            <p className="text-muted-foreground text-sm">
              {items.length
                ? `Showing ${search.offset + 1}–${search.offset + items.length}`
                : 'No skills on this page'}
            </p>
            <div className="flex gap-2">
              <Button
                variant="outline"
                disabled={searchPending || search.offset === 0}
                onClick={() =>
                  void router.navigate({
                    to: '/skills',
                    search: { ...search, offset: Math.max(0, search.offset - 50) },
                    replace: true,
                  })
                }
              >
                Previous
              </Button>
              <Button
                variant="outline"
                disabled={searchPending || items.length < 50}
                onClick={() =>
                  void router.navigate({
                    to: '/skills',
                    search: { ...search, offset: search.offset + 50 },
                    replace: true,
                  })
                }
              >
                Next
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>
      {uploadOpen ? (
        <SkillUploadDialog
          namespace={namespace}
          limits={limits}
          onClose={() => {
            setUploadOpen(false)
            void router.invalidate()
          }}
          onUploaded={uploaded}
        />
      ) : null}
    </div>
  )
}

function SkillCatalog({ items }: { items: SkillSummary[] }) {
  if (!items.length)
    return (
      <Empty>
        <EmptyHeader>
          <EmptyTitle>No skills found</EmptyTitle>
          <EmptyDescription>Upload a skill or try another search.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  return (
    <div className="min-w-0 overflow-hidden rounded-lg border">
      <Table>
        <TableHeader className="bg-muted/50">
          <TableRow>
            <TableHead>Skill</TableHead>
            <TableHead>Owner</TableHead>
            <TableHead>Latest</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {items.map((skill) => (
            <TableRow key={skill.id}>
              <TableCell className="max-w-xl whitespace-normal">
                <Link
                  to="/skills/$id"
                  params={{ id: skill.id }}
                  className="font-medium break-all underline-offset-4 hover:underline"
                >
                  {skill.name}
                </Link>
                <p className="text-muted-foreground mt-1 line-clamp-2 text-sm">
                  {skill.description}
                </p>
              </TableCell>
              <TableCell>{skill.namespace}</TableCell>
              <TableCell>v{skill.latest_version}</TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
