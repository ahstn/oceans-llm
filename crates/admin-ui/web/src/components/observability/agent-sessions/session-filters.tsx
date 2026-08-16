import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import type { AgentSessionFiltersInput } from '@/types/api'

type FilterKey = keyof AgentSessionFiltersInput

const textFields: Array<{ key: FilterKey; label: string }> = [
  { key: 'harness_key', label: 'Harness' },
  { key: 'requested_model_key', label: 'Model' },
  { key: 'operation', label: 'Operation' },
  { key: 'caller_class', label: 'Caller class' },
  { key: 'user_id', label: 'User ID' },
  { key: 'team_id', label: 'Team ID' },
  { key: 'service_account_id', label: 'Service account ID' },
  { key: 'session_source_id', label: 'Session source ID' },
  { key: 'session_source_hash', label: 'Session source hash' },
  { key: 'request_tag_key', label: 'Request tag key' },
  { key: 'request_tag_value', label: 'Request tag value' },
  { key: 'minimum_coverage_percent', label: 'Minimum coverage' },
]

const selectFields: Array<{
  key: FilterKey
  label: string
  options: Array<{ value: string; label: string }>
}> = [
  {
    key: 'lifecycle',
    label: 'Session state',
    options: [
      { value: 'open', label: 'Open' },
      { value: 'finalized', label: 'Finalized' },
    ],
  },
  {
    key: 'gateway_outcome',
    label: 'Outcome',
    options: ['succeeded', 'partial', 'failed', 'unknown'].map((value) => ({
      value,
      label: humanize(value),
    })),
  },
  {
    key: 'score_maturity',
    label: 'Score maturity',
    options: ['experimental', 'calibrated'].map((value) => ({ value, label: humanize(value) })),
  },
  {
    key: 'score_confidence',
    label: 'Score confidence',
    options: ['low', 'medium', 'high'].map((value) => ({ value, label: humanize(value) })),
  },
]

interface SessionFiltersProps {
  search: AgentSessionFiltersInput
  onChange: (key: FilterKey, value: string | undefined) => void
}

export function SessionFilters({ search, onChange }: SessionFiltersProps) {
  return (
    <details className="group rounded-md border">
      <summary className="cursor-pointer px-3 py-2 text-sm font-medium">Filters</summary>
      <div className="grid gap-3 border-t p-3 sm:grid-cols-2 lg:grid-cols-4">
        {selectFields.map((field) => (
          <div key={field.key} className="space-y-1.5">
            <Label>{field.label}</Label>
            <Select
              value={String(search[field.key] ?? 'all')}
              onValueChange={(value) => onChange(field.key, value === 'all' ? undefined : value)}
            >
              <SelectTrigger size="sm">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">All</SelectItem>
                {field.options.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        ))}
        {textFields.map((field) => (
          <div key={field.key} className="space-y-1.5">
            <Label htmlFor={`agent-session-${field.key}`}>{field.label}</Label>
            <Input
              id={`agent-session-${field.key}`}
              value={String(search[field.key] ?? '')}
              onChange={(event) => onChange(field.key, event.target.value || undefined)}
              className="h-8"
            />
          </div>
        ))}
        <DateField
          label="Started after"
          value={search.started_after}
          onChange={(value) =>
            onChange('started_after', value ? `${value}T00:00:00.000Z` : undefined)
          }
        />
        <DateField
          label="Started before"
          value={search.started_before}
          onChange={(value) =>
            onChange('started_before', value ? `${value}T23:59:59.999Z` : undefined)
          }
        />
      </div>
    </details>
  )
}

function DateField({
  label,
  value,
  onChange,
}: {
  label: string
  value?: string
  onChange: (value: string) => void
}) {
  const id = `agent-session-${label.toLowerCase().replace(' ', '-')}`
  return (
    <div className="space-y-1.5">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        type="date"
        value={value?.slice(0, 10) ?? ''}
        onChange={(event) => onChange(event.target.value)}
        className="h-8"
      />
    </div>
  )
}

export function ClearFiltersButton({
  visible,
  onClear,
}: {
  visible: boolean
  onClear: () => void
}) {
  return visible ? (
    <Button variant="ghost" size="sm" onClick={onClear}>
      Clear
    </Button>
  ) : null
}

function humanize(value: string) {
  return value.replaceAll('_', ' ').replace(/\b\w/g, (character) => character.toUpperCase())
}
