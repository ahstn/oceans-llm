import { useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import type { McpToolView } from '@/types/api'

export type ComboOption = {
  value: string
  label: string
  sublabel?: string
  keywords?: string
}

export type ToolGroup = {
  serverId: string
  serverName: string
  tools: McpToolView[]
}

/**
 * Single-select combobox over a flat option list. Backs the grant subject
 * picker, the single-tool target picker, and the toolset target picker — every
 * place a raw UUID input used to live.
 */
export function EntityComboBox({
  options,
  value,
  onChange,
  placeholder,
  searchPlaceholder,
  emptyText = 'No matches.',
  disabled = false,
  ariaLabel,
}: {
  options: ComboOption[]
  value: string
  onChange: (value: string) => void
  placeholder: string
  searchPlaceholder: string
  emptyText?: string
  disabled?: boolean
  ariaLabel?: string
}) {
  const [open, setOpen] = useState(false)
  const selected = options.find((option) => option.value === value)

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="secondary"
          aria-label={ariaLabel}
          className="w-full justify-between"
          disabled={disabled || options.length === 0}
        >
          <span className="truncate text-left">
            {options.length === 0 ? 'Nothing to select' : (selected?.label ?? placeholder)}
          </span>
          <span className="text-xs text-[var(--color-text-soft)]">▼</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[var(--radix-popover-trigger-width)] p-0">
        <Command>
          <CommandInput placeholder={searchPlaceholder} />
          <CommandList>
            <CommandEmpty>{emptyText}</CommandEmpty>
            <CommandGroup>
              {options.map((option) => (
                <CommandItem
                  key={option.value}
                  value={`${option.label} ${option.sublabel ?? ''} ${option.keywords ?? ''}`.trim()}
                  onSelect={() => {
                    onChange(option.value)
                    setOpen(false)
                  }}
                >
                  <span className="w-4 text-[var(--color-text-soft)]">
                    {option.value === value ? '✓' : ''}
                  </span>
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate font-medium">{option.label}</span>
                    {option.sublabel ? (
                      <span className="truncate font-mono text-xs text-[var(--color-text-muted)]">
                        {option.sublabel}
                      </span>
                    ) : null}
                  </div>
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}

/**
 * Multi-select tool picker grouped by server. Backs the toolset membership
 * editor — pick tools out of the live discovery catalog rather than pasting
 * UUIDs into a textarea.
 */
export function MultiToolPicker({
  groups,
  selectedIds,
  onToggle,
  disabled = false,
  buttonLabel = 'Select tools',
  searchPlaceholder = 'Search tools…',
  emptyText = 'No tools discovered.',
}: {
  groups: ToolGroup[]
  selectedIds: string[]
  onToggle: (toolId: string, checked: boolean) => void
  disabled?: boolean
  buttonLabel?: string
  searchPlaceholder?: string
  emptyText?: string
}) {
  const [open, setOpen] = useState(false)
  const selectedSet = new Set(selectedIds)
  const totalTools = groups.reduce((count, group) => count + group.tools.length, 0)

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="secondary"
          className="w-full justify-between"
          disabled={disabled || totalTools === 0}
        >
          <span className="truncate text-left">
            {totalTools === 0 ? 'No tools available' : summarize(selectedIds.length, buttonLabel)}
          </span>
          <span className="text-xs text-[var(--color-text-soft)]">▼</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[var(--radix-popover-trigger-width)] p-0">
        <Command>
          <CommandInput placeholder={searchPlaceholder} />
          <CommandList>
            <CommandEmpty>{emptyText}</CommandEmpty>
            {groups.map((group) => (
              <CommandGroup key={group.serverId} heading={group.serverName}>
                {group.tools.map((tool) => {
                  const isSelected = selectedSet.has(tool.id)
                  return (
                    <CommandItem
                      key={tool.id}
                      value={`${tool.display_name} ${tool.upstream_name} ${group.serverName}`.trim()}
                      disabled={disabled}
                      onSelect={() => onToggle(tool.id, !isSelected)}
                    >
                      <span className="w-4 text-[var(--color-text-soft)]">
                        {isSelected ? '✓' : ''}
                      </span>
                      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                        <span className="truncate font-medium">{tool.display_name}</span>
                        <span className="truncate font-mono text-xs text-[var(--color-text-muted)]">
                          {tool.upstream_name}
                        </span>
                      </div>
                    </CommandItem>
                  )
                })}
              </CommandGroup>
            ))}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}

/** Removable chip list for a current multi-tool selection. */
export function SelectedToolChips({
  toolIds,
  byId,
  onRemove,
}: {
  toolIds: string[]
  byId: Map<string, McpToolView>
  onRemove: (toolId: string) => void
}) {
  if (toolIds.length === 0) {
    return null
  }

  return (
    <div className="flex flex-wrap gap-2">
      {toolIds.map((toolId) => {
        const tool = byId.get(toolId)
        return (
          <Badge key={toolId} variant="secondary" className="gap-1">
            <span className="truncate">{tool?.display_name ?? toolId}</span>
            <button
              type="button"
              aria-label={`Remove ${tool?.display_name ?? toolId}`}
              className="text-[var(--color-text-soft)] hover:text-[var(--color-text)]"
              onClick={() => onRemove(toolId)}
            >
              ×
            </button>
          </Badge>
        )
      })}
    </div>
  )
}

function summarize(count: number, placeholder: string) {
  if (count === 0) {
    return placeholder
  }
  if (count === 1) {
    return '1 tool selected'
  }
  return `${count} tools selected`
}
