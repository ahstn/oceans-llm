import { useState, useTransition } from 'react'
import { createFileRoute, Link, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { PageHeader } from '@/components/layout/page-header'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Field, FieldLabel } from '@/components/ui/field'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Spinner } from '@/components/ui/spinner'
import { getSkill, saveSkillDefault } from '@/server/skills-data.functions'
import type { SkillUploadResponse, SkillSummary, SkillVersionDetail } from '@/types/skills-api'
import { SkillFiles } from './-files'
import { SkillUploadDialog } from './-upload-dialog'
import { SkillsErrorPage } from './-error'

export const Route = createFileRoute('/skills/$id')({
  validateSearch: (search: Record<string, unknown>): { version?: number } => ({
    version:
      typeof search.version === 'number' &&
      Number.isSafeInteger(search.version) &&
      search.version > 0
        ? search.version
        : undefined,
  }),
  loaderDeps: ({ search }) => search,
  loader: ({ params, deps }) => getSkill({ data: { id: params.id, version: deps.version } }),
  component: SkillDetailPage,
  errorComponent: SkillsErrorPage,
})

function SkillDetailPage() {
  const { detail, version, gatewayOrigin, limits } = Route.useLoaderData()
  const { session } = Route.useRouteContext()
  const router = useRouter()
  const [uploadOpen, setUploadOpen] = useState(false)
  const [pending, startTransition] = useTransition()
  const skill = detail.skill

  async function uploaded(next: SkillUploadResponse) {
    setUploadOpen(false)
    toast.success(
      `Version ${next.uploaded_version} uploaded. The default is still version ${next.skill.default_version}.`,
    )
    await router.invalidate()
    await router.navigate({
      to: '/skills/$id',
      params: { id: skill.id },
      search: { version: next.uploaded_version },
    })
  }

  function changeDefault() {
    startTransition(async () => {
      try {
        await saveSkillDefault({ data: { id: skill.id, version: version.version.version } })
        await router.invalidate()
        toast.success('Default version updated')
      } catch (error) {
        toast.error(
          error instanceof Error ? error.message : 'Unable to change the default version.',
        )
      }
    })
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6 [&_h1]:break-all">
      <div>
        <Button asChild variant="ghost" size="sm">
          <Link to="/skills" search={{ offset: 0 }}>
            Back to skills
          </Link>
        </Button>
      </div>
      <PageHeader
        section="Skills"
        title={`${skill.namespace}/${skill.name}`}
        description={version.manifest.description}
        actions={
          <SkillOwnerActions
            skill={skill}
            userId={session?.user.id ?? ''}
            version={version.version.version}
            pending={pending}
            onUpload={() => setUploadOpen(true)}
            onSetDefault={changeDefault}
          />
        }
      />
      <Card className="min-w-0">
        <CardHeader>
          <CardTitle>Version details</CardTitle>
          <CardDescription>
            Owned by {skill.namespace}. Version contents cannot be changed after upload.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex min-w-0 flex-col gap-5">
          <div className="flex flex-wrap items-end justify-between gap-4">
            <Field className="max-w-sm">
              <FieldLabel htmlFor="skill-version">Version</FieldLabel>
              <Select
                value={String(version.version.version)}
                onValueChange={(value) =>
                  void router.navigate({
                    to: '/skills/$id',
                    params: { id: skill.id },
                    search: { version: Number(value) },
                  })
                }
              >
                <SelectTrigger id="skill-version">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {detail.versions.map((item) => (
                      <SelectItem key={item.version} value={String(item.version)}>
                        Version {item.version}
                        {item.version === skill.default_version ? ' (default)' : ''}
                        {item.version === skill.latest_version ? ' (latest)' : ''}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            </Field>
            <Button asChild variant="outline">
              <a
                href={`${gatewayOrigin}/api/v1/skills/${skill.id}/versions/${version.version.version}/archive`}
              >
                Download ZIP
              </a>
            </Button>
          </div>
          <VersionMetadata detail={version} skill={skill} />
        </CardContent>
      </Card>
      <SkillFiles key={`${skill.id}:${version.version.version}`} id={skill.id} detail={version} />
      {uploadOpen ? (
        <SkillUploadDialog
          namespace={null}
          limits={limits}
          skill={skill}
          onClose={() => setUploadOpen(false)}
          onUploaded={uploaded}
        />
      ) : null}
    </div>
  )
}

export function SkillOwnerActions({
  skill,
  userId,
  version,
  pending,
  onUpload,
  onSetDefault,
}: {
  skill: SkillSummary
  userId: string
  version: number
  pending: boolean
  onUpload: () => void
  onSetDefault: () => void
}) {
  if (skill.owner_user_id !== userId) return null
  return (
    <div className="flex flex-wrap gap-2">
      <Button
        variant="outline"
        disabled={pending || version === skill.default_version}
        onClick={onSetDefault}
      >
        {pending ? <Spinner data-icon="inline-start" /> : null}Set as default
      </Button>
      <Button disabled={pending} onClick={onUpload}>
        Upload new version
      </Button>
    </div>
  )
}

function VersionMetadata({ detail, skill }: { detail: SkillVersionDetail; skill: SkillSummary }) {
  return (
    <dl className="grid min-w-0 gap-4 text-sm sm:grid-cols-2">
      <div>
        <dt className="text-muted-foreground">Status</dt>
        <dd className="mt-1 flex gap-2">
          {detail.version.version === skill.default_version ? (
            <Badge>Default</Badge>
          ) : (
            <Badge variant="outline">Version {detail.version.version}</Badge>
          )}
          {detail.version.version === skill.latest_version ? (
            <Badge variant="secondary">Latest</Badge>
          ) : null}
        </dd>
      </div>
      <div>
        <dt className="text-muted-foreground">Uploaded</dt>
        <dd>
          <time dateTime={detail.version.created_at}>
            {new Date(detail.version.created_at).toLocaleString('en-GB', { timeZone: 'UTC' })} UTC
          </time>
        </dd>
      </div>
      <div>
        <dt className="text-muted-foreground">Contents</dt>
        <dd>
          {detail.version.file_count.toLocaleString()} files ·{' '}
          {detail.version.archive_bytes.toLocaleString()} bytes archived ·{' '}
          {detail.version.extracted_bytes.toLocaleString()} bytes expanded
        </dd>
      </div>
      <div>
        <dt className="text-muted-foreground">License</dt>
        <dd>{detail.manifest.license ?? 'Not specified'}</dd>
      </div>
      <div className="min-w-0 sm:col-span-2">
        <dt className="text-muted-foreground">SHA-256</dt>
        <dd className="font-mono break-all">{detail.version.sha256}</dd>
      </div>
      {detail.manifest.compatibility ? (
        <div className="sm:col-span-2">
          <dt className="text-muted-foreground">Compatibility</dt>
          <dd>{detail.manifest.compatibility}</dd>
        </div>
      ) : null}
      {detail.manifest['allowed-tools'] ? (
        <div className="sm:col-span-2">
          <dt className="text-muted-foreground">Allowed tools</dt>
          <dd className="font-mono break-words">{detail.manifest['allowed-tools']}</dd>
        </div>
      ) : null}
    </dl>
  )
}
