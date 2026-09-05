import { useState, type FormEvent } from 'react'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
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
import { Textarea } from '@/components/ui/textarea'
import type { McpToolsetView } from '@/types/api'
import { selectedTools, type ToolsetDetails } from './model'

export type DialogMode = 'create' | 'edit' | 'review' | 'disable' | 'access' | null
interface Props {
  mode: DialogMode
  selected: McpToolsetView | null
  draftIds: string[]
  onClose: () => void
  onSaveDetails: (details: ToolsetDetails) => boolean
  onConfirm: () => void
}

export function ToolsetDialogs(props: Props) {
  return (
    <Dialog
      open={props.mode !== null}
      onOpenChange={(open) => {
        if (!open) props.onClose()
      }}
    >
      <DialogContent className="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-lg">
        {props.mode === 'create' || props.mode === 'edit' ? (
          <DetailsForm {...props} />
        ) : props.mode === 'review' ? (
          <ReplacementReview {...props} />
        ) : props.mode === 'access' ? (
          <AccessInfo onClose={props.onClose} />
        ) : (
          <ConfirmAction {...props} />
        )}
      </DialogContent>
    </Dialog>
  )
}

function DetailsForm(props: Props) {
  const creating = props.mode === 'create'
  const [details, setDetails] = useState<ToolsetDetails>({
    display_name: creating ? '' : (props.selected?.display_name ?? ''),
    toolset_key: creating ? '' : (props.selected?.toolset_key ?? ''),
    description: creating ? '' : (props.selected?.description ?? ''),
  })
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!details.display_name.trim() || !details.toolset_key.trim()) return
    props.onSaveDetails({
      display_name: details.display_name.trim(),
      toolset_key: details.toolset_key.trim(),
      description: details.description?.trim() || null,
    })
  }
  return (
    <>
      <DialogHeader>
        <DialogTitle>{creating ? 'New tool set' : 'Edit tool set details'}</DialogTitle>
        <DialogDescription>
          {creating
            ? 'Give this collection a clear purpose. Choose its tools next.'
            : 'Update the name and purpose of this collection.'}
        </DialogDescription>
      </DialogHeader>
      <form onSubmit={submit} className="flex flex-col gap-6">
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="set-name">Display name</FieldLabel>
            <Input
              id="set-name"
              required
              autoFocus
              value={details.display_name}
              onChange={(event) => setDetails({ ...details, display_name: event.target.value })}
              placeholder="Engineering essentials"
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="set-key">Key</FieldLabel>
            <Input
              id="set-key"
              required
              readOnly={!creating}
              value={details.toolset_key}
              onChange={(event) => setDetails({ ...details, toolset_key: event.target.value })}
              placeholder="engineering-essentials"
            />
            <FieldDescription>
              A stable identifier for access rules. It cannot be changed later.
            </FieldDescription>
          </Field>
          <Field>
            <FieldLabel htmlFor="set-description">Description</FieldLabel>
            <Textarea
              id="set-description"
              rows={3}
              value={details.description ?? ''}
              onChange={(event) => setDetails({ ...details, description: event.target.value })}
              placeholder="Which task should this set support?"
            />
          </Field>
        </FieldGroup>
        <DialogFooter>
          <Button type="button" variant="outline" onClick={props.onClose}>
            Cancel
          </Button>
          <Button type="submit">{creating ? 'Create tool set' : 'Save details'}</Button>
        </DialogFooter>
      </form>
    </>
  )
}

function ReplacementReview(props: Props) {
  const tools = selectedTools(props.draftIds)
  return (
    <>
      <DialogHeader>
        <DialogTitle>
          {tools.length === 0 ? 'Remove all tools?' : 'Save tool selection?'}
        </DialogTitle>
        <DialogDescription>
          Review the complete selection for {props.selected?.display_name}.
        </DialogDescription>
      </DialogHeader>
      <Alert variant={tools.length === 0 ? 'destructive' : 'default'}>
        <AlertTitle>
          {tools.length === 0
            ? 'This will remove every tool'
            : `${tools.length} tools in this selection`}
        </AlertTitle>
        <AlertDescription>
          {tools.length === 0
            ? 'The saved set will have no tools. Its metadata and access rules will remain.'
            : 'This list includes saved tools and your current changes. Saving applies the complete selection.'}
        </AlertDescription>
      </Alert>
      {tools.length > 0 ? (
        <ul className="flex max-h-72 flex-col gap-2 overflow-y-auto rounded-lg border p-3">
          {tools.map((tool) => (
            <li key={tool.id} className="flex flex-col gap-1">
              <span className="text-sm">{tool.display_name}</span>
              <span className="text-muted-foreground font-mono text-xs wrap-anywhere">
                {tool.server_id} / {tool.upstream_name}
              </span>
            </li>
          ))}
        </ul>
      ) : null}
      <p className="text-muted-foreground text-xs">
        Sample preview only. No gateway data will change.
      </p>
      <DialogFooter>
        <Button variant="outline" onClick={props.onClose}>
          Keep editing
        </Button>
        <Button variant={tools.length === 0 ? 'destructive' : 'default'} onClick={props.onConfirm}>
          {tools.length === 0 ? 'Remove all tools' : 'Save tools'}
        </Button>
      </DialogFooter>
    </>
  )
}

function ConfirmAction(props: Props) {
  return (
    <>
      <DialogHeader>
        <DialogTitle>Disable {props.selected?.display_name}?</DialogTitle>
        <DialogDescription>
          The tool set will be disabled. Its details remain available for reference.
        </DialogDescription>
      </DialogHeader>
      <DialogFooter>
        <Button variant="outline" onClick={props.onClose}>
          Cancel
        </Button>
        <Button variant="destructive" onClick={props.onConfirm}>
          Disable tool set
        </Button>
      </DialogFooter>
    </>
  )
}

function AccessInfo({ onClose }: { onClose: () => void }) {
  return (
    <>
      <DialogHeader>
        <DialogTitle>Manage access</DialogTitle>
        <DialogDescription>
          Tool sets and access rules are separate steps in the MCP workspace.
        </DialogDescription>
      </DialogHeader>
      <ol className="flex list-decimal flex-col gap-4 pl-5 text-sm">
        <li>Save the tool selection for your set.</li>
        <li>Open Access and choose a person, team, service account, or API key.</li>
        <li>Create a grant for the tool set with the required permission.</li>
      </ol>
      <p className="text-muted-foreground text-sm">
        This design study covers Tool Sets. The live Access page remains available beside Servers
        and Tool Sets.
      </p>
      <DialogFooter>
        <Button onClick={onClose}>Done</Button>
      </DialogFooter>
    </>
  )
}
