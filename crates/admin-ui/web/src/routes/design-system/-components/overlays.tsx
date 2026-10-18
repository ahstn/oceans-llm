import { HugeiconsIcon } from '@hugeicons/react'
import {
  ArrowDown02Icon,
  ArrowUp02Icon,
  Copy01Icon,
  Delete02Icon,
  InformationCircleIcon,
  Key01Icon,
  MoreHorizontalIcon,
  Settings02Icon,
  SlidersHorizontalIcon,
  UnfoldMoreIcon,
  UserCircleIcon,
} from '@hugeicons/core-free-icons'
import { useState } from 'react'

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
} from '@/components/ui/command'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from '@/components/ui/popover'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from '@/components/ui/sheet'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

import { ShowcaseBlock, ShowcaseRow, ShowcaseSection } from './showcase'

export function OverlaysSection() {
  return (
    <ShowcaseSection
      id="overlays"
      title="Overlays"
      description="Floating layers use --popover, a step above cards, with a hairline ring. Menus and selects cast the popover shadow; dialogs and sheets cast the deeper dialog shadow and always have a title."
    >
      <div className="grid gap-4 lg:grid-cols-2">
        <ShowcaseBlock
          title="Modal surfaces"
          description="Dialog for forms, AlertDialog for irreversible actions, Sheet for detail."
        >
          <ShowcaseRow label="Open one">
            <CreateKeyDialog />
            <RevokeKeyAlert />
            <Sheet>
              <SheetTrigger asChild>
                <Button variant="outline">Open sheet</Button>
              </SheetTrigger>
              <SheetContent>
                <SheetHeader>
                  <SheetTitle>ci-pipeline</SheetTitle>
                  <SheetDescription>Created by Alice, owned by Platform.</SheetDescription>
                </SheetHeader>
                <p className="text-muted-foreground px-4 font-mono text-sm">key_01JB7Q4M</p>
                <SheetFooter>
                  <Button variant="outline">Rotate key</Button>
                </SheetFooter>
              </SheetContent>
            </Sheet>
          </ShowcaseRow>
        </ShowcaseBlock>

        <ShowcaseBlock
          title="Anchored layers"
          description="Popover for small forms, Tooltip for icon labels, DropdownMenu for row actions and column menus. A column menu's Columns submenu toggles columns with checkbox items."
        >
          <ShowcaseRow label="Open one">
            <Popover>
              <PopoverTrigger asChild>
                <Button variant="outline">
                  <HugeiconsIcon icon={InformationCircleIcon} data-icon="inline-start" />
                  Popover
                </Button>
              </PopoverTrigger>
              <PopoverContent>
                <PopoverHeader>
                  <PopoverTitle>How budgets apply</PopoverTitle>
                  <PopoverDescription>
                    A key spends from its owner&apos;s budget. Team budgets cap every member.
                  </PopoverDescription>
                </PopoverHeader>
              </PopoverContent>
            </Popover>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="icon" aria-label="Copy key ID">
                  <HugeiconsIcon icon={Copy01Icon} />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Copy key ID</TooltipContent>
            </Tooltip>
            <RowActionsMenu />
            <ColumnMenu />
          </ShowcaseRow>
        </ShowcaseBlock>
      </div>

      <ShowcaseBlock
        title="Command"
        description="Shown inline here; in the app it opens inside a Dialog on ⌘K."
      >
        <InlineCommand />
      </ShowcaseBlock>
    </ShowcaseSection>
  )
}

function CreateKeyDialog() {
  return (
    <Dialog>
      <DialogTrigger asChild>
        <Button>Open dialog</Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create API key</DialogTitle>
          <DialogDescription>The secret is shown once. Store it before closing.</DialogDescription>
        </DialogHeader>
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="ds-dialog-name">Key name</FieldLabel>
            <Input id="ds-dialog-name" placeholder="ci-pipeline" />
          </Field>
        </FieldGroup>
        <DialogFooter>
          <DialogClose asChild>
            <Button variant="outline">Cancel</Button>
          </DialogClose>
          <Button>Create key</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function RevokeKeyAlert() {
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button variant="destructive">Open alert dialog</Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogMedia>
            <HugeiconsIcon icon={Delete02Icon} />
          </AlertDialogMedia>
          <AlertDialogTitle>Revoke ci-pipeline?</AlertDialogTitle>
          <AlertDialogDescription>
            Requests using this key start failing immediately. This can&apos;t be undone.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction variant="destructive">Revoke key</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

const columns = ['Model', 'Latency', 'Cost'] as const

/** A table column's menu, as the data grid builds it: sorting, then a Columns submenu. */
function ColumnMenu() {
  const [shown, setShown] = useState<ReadonlySet<string>>(() => new Set(['Model', 'Latency']))

  function toggle(column: string, checked: boolean) {
    setShown((current) => {
      const next = new Set(current)
      if (checked) {
        next.add(column)
      } else {
        next.delete(column)
      }
      return next
    })
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm">
          Latency
          <HugeiconsIcon icon={UnfoldMoreIcon} data-icon="inline-end" />
        </Button>
      </DropdownMenuTrigger>
      {/* The menu otherwise takes the header's width; the data grid sizes it the same way. */}
      <DropdownMenuContent className="w-40" align="start">
        <DropdownMenuGroup>
          <DropdownMenuItem>
            <HugeiconsIcon icon={ArrowUp02Icon} />
            Sort ascending
          </DropdownMenuItem>
          <DropdownMenuItem>
            <HugeiconsIcon icon={ArrowDown02Icon} />
            Sort descending
          </DropdownMenuItem>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuSub>
          <DropdownMenuSubTrigger>
            <HugeiconsIcon icon={SlidersHorizontalIcon} />
            Columns
          </DropdownMenuSubTrigger>
          <DropdownMenuSubContent>
            <DropdownMenuGroup>
              {columns.map((column) => (
                <DropdownMenuCheckboxItem
                  key={column}
                  checked={shown.has(column)}
                  // Keep the menu open, so several columns can be toggled in a row.
                  onSelect={(event) => event.preventDefault()}
                  onCheckedChange={(checked) => toggle(column, checked)}
                >
                  {column}
                </DropdownMenuCheckboxItem>
              ))}
            </DropdownMenuGroup>
          </DropdownMenuSubContent>
        </DropdownMenuSub>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function RowActionsMenu() {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" aria-label="Row actions">
          <HugeiconsIcon icon={MoreHorizontalIcon} />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuLabel>ci-pipeline</DropdownMenuLabel>
        <DropdownMenuGroup>
          <DropdownMenuItem>
            <HugeiconsIcon icon={Copy01Icon} />
            Copy key ID
            <DropdownMenuShortcut>⌘C</DropdownMenuShortcut>
          </DropdownMenuItem>
          <DropdownMenuItem>
            <HugeiconsIcon icon={Settings02Icon} />
            Edit
          </DropdownMenuItem>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuItem variant="destructive">
            <HugeiconsIcon icon={Delete02Icon} />
            Revoke
          </DropdownMenuItem>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function InlineCommand() {
  // Controlled, so cmdk doesn't scroll its selected item into view and jump the page on load.
  const [value, setValue] = useState('')

  return (
    <Command value={value} onValueChange={setValue} className="rounded-lg border">
      <CommandInput placeholder="Jump to a page or key…" />
      <CommandList>
        <CommandEmpty>No results.</CommandEmpty>
        <CommandGroup heading="Pages">
          <CommandItem>
            <HugeiconsIcon icon={UserCircleIcon} />
            Profile
          </CommandItem>
          <CommandItem>
            <HugeiconsIcon icon={Key01Icon} />
            API Keys
            <CommandShortcut>G K</CommandShortcut>
          </CommandItem>
        </CommandGroup>
        <CommandSeparator />
        <CommandGroup heading="Settings">
          <CommandItem>
            <HugeiconsIcon icon={Settings02Icon} />
            Spend controls
          </CommandItem>
        </CommandGroup>
      </CommandList>
    </Command>
  )
}
