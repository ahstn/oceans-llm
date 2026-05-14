import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'

export type EntityTag = {
  key: string
  value: string
}

export function EntityTagsField({
  label,
  tags,
  onChange,
}: {
  label: string
  tags: EntityTag[]
  onChange: (tags: EntityTag[]) => void
}) {
  function updateTag(index: number, field: 'key' | 'value', value: string) {
    onChange(tags.map((tag, tagIndex) => (tagIndex === index ? { ...tag, [field]: value } : tag)))
  }

  return (
    <Field>
      <FieldLabel>{label}</FieldLabel>
      <div className="flex flex-col gap-2">
        {tags.map((tag, index) => (
          <div key={index} className="grid gap-2 sm:grid-cols-[1fr_1fr_auto]">
            <Input
              value={tag.key}
              onChange={(event) => updateTag(index, 'key', event.target.value)}
              placeholder="cost-center"
            />
            <Input
              value={tag.value}
              onChange={(event) => updateTag(index, 'value', event.target.value)}
              placeholder="platform"
            />
            <Button
              type="button"
              variant="ghost"
              onClick={() => onChange(tags.filter((_, tagIndex) => tagIndex !== index))}
            >
              Remove
            </Button>
          </div>
        ))}
        <Button
          type="button"
          variant="secondary"
          onClick={() => onChange([...tags, { key: '', value: '' }])}
          disabled={tags.length >= 5}
        >
          Add tag
        </Button>
      </div>
      <FieldDescription>
        Up to five lowercase key/value tags. Keys may use letters, digits, dot, underscore, or dash.
      </FieldDescription>
    </Field>
  )
}

export function EntityTagBadges({ tags }: { tags: EntityTag[] }) {
  if (tags.length === 0) {
    return <span className="text-xs text-[var(--color-text-soft)]">No tags</span>
  }

  return (
    <div className="flex flex-wrap gap-2">
      {tags.map((tag) => (
        <Badge key={tag.key} variant="secondary">
          {tag.key}:{tag.value}
        </Badge>
      ))}
    </div>
  )
}

export function sanitizeEntityTags(tags: EntityTag[]) {
  return tags
    .map((tag) => ({ key: tag.key.trim(), value: tag.value.trim() }))
    .filter((tag) => tag.key.length > 0 || tag.value.length > 0)
}
