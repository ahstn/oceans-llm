import { useState, type ReactNode } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { EMPTY_VALUE } from '@/lib/format'
import { cn } from '@/lib/utils'
import type { EntityTag } from '@/routes/identity/-entity-tag-utils'

export function EntityTagsField({
  label,
  tags,
  onChange,
}: {
  label: string
  tags: EntityTag[] | null | undefined
  onChange: (tags: EntityTag[]) => void
}) {
  const rows = tags ?? []
  function updateTag(index: number, field: 'key' | 'value', value: string) {
    onChange(rows.map((tag, tagIndex) => (tagIndex === index ? { ...tag, [field]: value } : tag)))
  }

  return (
    <Field>
      <FieldLabel>{label}</FieldLabel>
      <div className="flex flex-col gap-2">
        {rows.map((tag, index) => (
          // The API tag shape has no ID, and its array position is its edit identity.
          // oxlint-disable-next-line react/no-array-index-key
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
              onClick={() => onChange(rows.filter((_, tagIndex) => tagIndex !== index))}
            >
              Remove
            </Button>
          </div>
        ))}
        <Button
          type="button"
          variant="secondary"
          onClick={() => onChange([...rows, { key: '', value: '' }])}
          disabled={rows.length >= 5}
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
    return <span className="text-muted-foreground text-xs">No tags</span>
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

// A clickable chip takes the lilac action hover, so it reads as a filter shortcut.
const CHIP_ACTION =
  'hover:border-primary/55 hover:bg-primary/8 focus-visible:ring-ring/50 cursor-pointer outline-none focus-visible:ring-3'

/** One `key:value` tag as a single pill: the key muted, the value bright, in a mono face. */
export function EntityTagChip({
  tag,
  onSelect,
}: {
  tag: EntityTag
  /** Makes the chip a button that filters by this tag. */
  onSelect?: (tag: EntityTag) => void
}) {
  const content = (
    <>
      <span className="bg-foreground/3 text-muted-foreground flex max-w-28 min-w-0 shrink-0 items-center pr-1.5 pl-2">
        <span className="truncate">{tag.key}</span>
      </span>
      <span className="border-border flex min-w-0 items-center border-l pr-2 pl-1.5">
        <span className="truncate">{tag.value}</span>
      </span>
    </>
  )
  const className =
    'border-border inline-flex h-5 max-w-52 min-w-0 items-stretch overflow-hidden rounded-full border font-mono text-xs whitespace-nowrap transition-colors'
  if (!onSelect) {
    return (
      <span className={className} title={`${tag.key}:${tag.value}`}>
        {content}
      </span>
    )
  }
  return (
    <button
      type="button"
      className={cn(className, CHIP_ACTION)}
      title={`Filter by ${tag.key}:${tag.value}`}
      aria-label={`Filter by ${tag.key}:${tag.value}`}
      onClick={() => onSelect(tag)}
    >
      {content}
    </button>
  )
}

const EMPTY_TAGS = <span className="text-muted-foreground">{EMPTY_VALUE}</span>

/**
 * Tags on one line: the first `limit` as chips, then a `+N` chip whose popover lists every tag.
 * Rows stay one line tall however many tags an entity carries.
 */
export function EntityTagChips({
  tags,
  limit = 2,
  onSelect,
  empty = EMPTY_TAGS,
}: {
  tags: EntityTag[]
  limit?: number
  onSelect?: (tag: EntityTag) => void
  empty?: ReactNode
}) {
  const [open, setOpen] = useState(false)
  if (tags.length === 0) return <>{empty}</>

  const hidden = tags.length - limit
  return (
    <div className="flex min-w-0 items-center gap-1.5">
      {tags.slice(0, limit).map((tag) => (
        <EntityTagChip key={tag.key} tag={tag} onSelect={onSelect} />
      ))}
      {hidden > 0 ? (
        <Popover open={open} onOpenChange={setOpen}>
          <PopoverTrigger asChild>
            <button
              type="button"
              aria-label={`Show all ${tags.length} tags`}
              className={cn(
                'border-border text-muted-foreground hover:text-foreground data-[state=open]:border-primary/55 data-[state=open]:bg-primary/8 data-[state=open]:text-foreground inline-flex h-5 shrink-0 items-center rounded-full border px-2 text-xs font-medium tabular-nums transition-colors',
                CHIP_ACTION,
              )}
            >
              +{hidden}
            </button>
          </PopoverTrigger>
          <PopoverContent align="start" className="w-auto max-w-72 gap-2 p-2">
            <p className="text-muted-foreground text-xs">
              {tags.length} tags{onSelect ? ' · choose one to filter' : ''}
            </p>
            <div className="flex flex-col items-start gap-1.5">
              {tags.map((tag) => (
                <EntityTagChip
                  key={tag.key}
                  tag={tag}
                  onSelect={
                    onSelect
                      ? (selected) => {
                          setOpen(false)
                          onSelect(selected)
                        }
                      : undefined
                  }
                />
              ))}
            </div>
          </PopoverContent>
        </Popover>
      ) : null}
    </div>
  )
}
