import { useState, useTransition } from 'react'

import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Spinner } from '@/components/ui/spinner'
import { saveSkillNamespace, uploadSkill } from '@/server/skills-data.functions'
import type {
  SkillUploadResponse,
  SkillNamespace,
  SkillSummary,
  SkillLimits,
} from '@/types/skills-api'

export function SkillUploadDialog({
  namespace,
  limits,
  skill,
  onClose,
  onUploaded,
}: {
  namespace: SkillNamespace | null
  limits: SkillLimits
  skill?: SkillSummary
  onClose: () => void
  onUploaded: (detail: SkillUploadResponse) => Promise<void>
}) {
  const [claimedNamespace, setClaimedNamespace] = useState(namespace)
  const [handle, setHandle] = useState('')
  const [archive, setArchive] = useState<File | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [pending, startTransition] = useTransition()
  const needsNamespace = !claimedNamespace && !skill

  function submit() {
    setError(null)
    startTransition(async () => {
      try {
        if (needsNamespace) {
          setClaimedNamespace(await saveSkillNamespace({ data: { handle } }))
          return
        }
        if (!archive) throw new Error('Choose a ZIP archive.')
        if (archive.size === 0) throw new Error('The archive is empty.')
        if (archive.size > limits.max_archive_bytes)
          throw new Error(
            `The archive must be ${formatBytes(limits.max_archive_bytes)} or smaller.`,
          )
        const data = new FormData()
        data.set('archive', archive)
        if (skill) data.set('id', skill.id)
        const detail = await uploadSkill({ data })
        await onUploaded(detail)
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : 'Unable to upload this skill.')
      }
    })
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !pending) onClose()
      }}
    >
      <DialogContent className="min-w-0 overflow-hidden">
        <form
          onSubmit={(event) => {
            event.preventDefault()
            submit()
          }}
          className="flex min-w-0 flex-col gap-5"
        >
          <DialogHeader>
            <DialogTitle>
              {needsNamespace
                ? 'Choose your skill namespace'
                : skill
                  ? 'Upload a new version'
                  : 'Upload a skill'}
            </DialogTitle>
            <DialogDescription>
              {needsNamespace
                ? 'Your namespace identifies your skills, such as alex/code-review. You cannot change it after it is claimed.'
                : skill
                  ? `Add an immutable version of ${skill.namespace}/${skill.name}. The default version will stay unchanged.`
                  : `Your skill will be stored under ${claimedNamespace?.handle}. All authenticated users can read it.`}
            </DialogDescription>
          </DialogHeader>
          <FieldGroup>
            {needsNamespace ? (
              <Field>
                <FieldLabel htmlFor="skill-namespace">Namespace</FieldLabel>
                <Input
                  id="skill-namespace"
                  value={handle}
                  onChange={(event) => setHandle(event.target.value)}
                  required
                  maxLength={64}
                  autoComplete="off"
                  disabled={pending}
                />
                <FieldDescription>
                  Use lowercase letters, numbers, and single hyphens. The name must be unique.
                </FieldDescription>
              </Field>
            ) : (
              <ArchiveField pending={pending} limits={limits} onChange={setArchive} />
            )}
          </FieldGroup>
          {error ? (
            <Alert variant="destructive">
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          ) : null}
          <DialogFooter>
            <Button type="button" variant="outline" disabled={pending} onClick={onClose}>
              Cancel
            </Button>
            <Button
              type="submit"
              disabled={pending || (needsNamespace ? !handle.trim() : !archive)}
            >
              {pending ? <Spinner data-icon="inline-start" /> : null}
              {needsNamespace ? 'Claim namespace' : 'Upload skill'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function ArchiveField({
  pending,
  onChange,
  limits,
}: {
  pending: boolean
  limits: SkillLimits
  onChange: (file: File | null) => void
}) {
  return (
    <Field>
      <FieldLabel htmlFor="skill-archive">ZIP archive</FieldLabel>
      <Input
        id="skill-archive"
        type="file"
        accept=".zip,application/zip"
        onChange={(event) => onChange(event.target.files?.[0] ?? null)}
        disabled={pending}
        required
      />
      <FieldDescription>
        Include SKILL.md and any scripts, references, or assets. Limits:{' '}
        {formatBytes(limits.max_archive_bytes)} ZIP, {formatBytes(limits.max_expanded_bytes)}{' '}
        expanded, and {limits.max_files.toLocaleString()} files. Scripts are never run during
        upload.
      </FieldDescription>
    </Field>
  )
}

function formatBytes(bytes: number) {
  return bytes >= 1024 * 1024
    ? `${(bytes / (1024 * 1024)).toLocaleString()} MiB`
    : `${bytes.toLocaleString()} bytes`
}
