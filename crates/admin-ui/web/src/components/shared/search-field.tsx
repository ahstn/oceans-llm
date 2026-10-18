import { Cancel01Icon, Search01Icon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from '@/components/ui/input-group'

/** Free-text filter that applies on Enter; keyed by its value so URL changes reset the input. */
export function SearchField({
  label,
  value,
  disabled,
  onSubmit,
}: {
  label: string
  value: string | null | undefined
  disabled: boolean
  onSubmit: (value: string) => void
}) {
  return (
    <form
      key={value ?? ''}
      className="w-full sm:w-56"
      action={(formData) => {
        const next = formData.get('value')
        onSubmit(typeof next === 'string' ? next.trim() : '')
      }}
    >
      <InputGroup>
        <InputGroupInput
          name="value"
          aria-label={label}
          placeholder={`${label}…`}
          defaultValue={value ?? ''}
          disabled={disabled}
        />
        <InputGroupAddon>
          <AppIcon icon={Search01Icon} aria-hidden />
        </InputGroupAddon>
        {value ? (
          <InputGroupAddon align="inline-end">
            <InputGroupButton
              aria-label={`Clear ${label.toLowerCase()}`}
              size="icon-xs"
              disabled={disabled}
              onClick={() => onSubmit('')}
            >
              <AppIcon icon={Cancel01Icon} aria-hidden />
            </InputGroupButton>
          </InputGroupAddon>
        ) : null}
      </InputGroup>
    </form>
  )
}
