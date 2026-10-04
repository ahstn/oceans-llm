import { useState } from 'react'
import { createFileRoute, Link, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { PageHeader } from '@/components/layout/page-header'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
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
  validateSearch: (search: Record<string, unknown>): { offset: number; namespace?: string } => ({
    offset:
      typeof search.offset === 'number' && Number.isSafeInteger(search.offset) && search.offset > 0
        ? search.offset
        : 0,
    namespace:
      typeof search.namespace === 'string' && search.namespace.trim()
        ? search.namespace.trim()
        : undefined,
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
  const [namespaceFilter, setNamespaceFilter] = useState(search.namespace ?? '')

  async function uploaded(detail: SkillDetail) {
    setUploadOpen(false)
    toast.success('Skill uploaded')
    await router.invalidate()
    await router.navigate({ to: '/skills/$id', params: { id: detail.skill.id } })
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Control Plane"
        title="Skills"
        description="Share re-usable agent skills. All users can fetch or add new skills."
        actions={<Button onClick={() => setUploadOpen(true)}>Upload skill</Button>}
      />
      <Card className="min-w-0">
        <CardHeader>
          <CardTitle>Skill catalog</CardTitle>
          <CardDescription>Filter, view and download existing skills.</CardDescription>
        </CardHeader>
        <CardContent className="flex min-w-0 flex-col gap-5">
          <form
            className="flex flex-wrap items-end gap-3"
            onSubmit={(event) => {
              event.preventDefault()
              void router.navigate({
                to: '/skills',
                search: { offset: 0, namespace: namespaceFilter.trim() || undefined },
              })
            }}
          >
            <Field className="max-w-sm">
              <FieldLabel htmlFor="filter-namespace">Owner namespace</FieldLabel>
              <Input
                id="filter-namespace"
                placeholder="All namespaces"
                value={namespaceFilter}
                onChange={(event) => setNamespaceFilter(event.target.value)}
              />
            </Field>
            <Button type="submit" variant="outline">
              Filter
            </Button>
          </form>
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
                disabled={search.offset === 0}
                onClick={() =>
                  void router.navigate({
                    to: '/skills',
                    search: { ...search, offset: Math.max(0, search.offset - 50) },
                  })
                }
              >
                Previous
              </Button>
              <Button
                variant="outline"
                disabled={items.length < 50}
                onClick={() =>
                  void router.navigate({
                    to: '/skills',
                    search: { ...search, offset: search.offset + 50 },
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
          <EmptyDescription>Upload a skill or try another namespace.</EmptyDescription>
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
