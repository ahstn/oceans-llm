import { useState, useTransition, type Dispatch, type FormEvent, type SetStateAction } from 'react'
import { useRouter } from '@tanstack/react-router'
import { Alert02Icon } from '@hugeicons/core-free-icons'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
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
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { getErrorMessage } from '@/lib/errors'
import { cn } from '@/lib/utils'
import { removeBudget, saveBudget } from '@/server/admin-data.functions'
import type { BudgetScopeRequest } from '@/types/api'

import {
  budgetPayload,
  INHERITED_BUDGET_WARNING,
  initialBudgetSettings,
  isInheritedBudgetSource,
  settingsFromBudget,
  type BudgetSettingsForm,
  type BudgetTarget,
} from './-budget-model'

// One editor owns the dialog, its draft form, and every budget mutation on the page.

export interface BudgetEditor {
  /** The budget open in the dialog, or `null` when it is closed. */
  target: BudgetTarget | null
  form: BudgetSettingsForm
  setForm: Dispatch<SetStateAction<BudgetSettingsForm>>
  isPending: boolean
  open: (target: BudgetTarget) => void
  close: () => void
  /** Submits the dialog form for `target`. */
  save: (event: FormEvent<HTMLFormElement>) => void
  /** Validates `settings`, saves them for `scope`, and runs `after` on success. */
  upsert: (
    scope: BudgetScopeRequest,
    settings: BudgetSettingsForm,
    message: string,
    after?: () => void,
  ) => void
  remove: (scope: BudgetScopeRequest, message: string) => void
}

export function useBudgetEditor(): BudgetEditor {
  const router = useRouter()
  const [target, setTarget] = useState<BudgetTarget | null>(null)
  const [form, setForm] = useState<BudgetSettingsForm>(initialBudgetSettings)
  const [isPending, startTransition] = useTransition()

  function close() {
    setTarget(null)
    setForm(initialBudgetSettings)
  }

  function runMutation(mutation: () => Promise<unknown>, message: string, after?: () => void) {
    startTransition(async () => {
      try {
        await mutation()
        toast.success(message)
        await router.invalidate()
        after?.()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  function upsert(
    scope: BudgetScopeRequest,
    settings: BudgetSettingsForm,
    message: string,
    after?: () => void,
  ) {
    const result = budgetPayload(scope, settings)
    if (!result.ok) {
      toast.error(result.error)
      return
    }
    runMutation(() => saveBudget({ data: result.payload }), message, after)
  }

  return {
    target,
    form,
    setForm,
    isPending,
    close,
    upsert,
    open(next) {
      setTarget(next)
      setForm(settingsFromBudget(next.budget))
    },
    save(event) {
      event.preventDefault()
      if (target) upsert(target.scope, form, 'Budget updated', close)
    },
    remove(scope, message) {
      runMutation(() => removeBudget({ data: { scope } }), message)
    },
  }
}

export function BudgetDialog({ editor }: { editor: BudgetEditor }) {
  const { target } = editor
  return (
    <Dialog open={target !== null} onOpenChange={(open) => (open ? null : editor.close())}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Configure budget</DialogTitle>
          <DialogDescription>
            Set the cadence, limit, and hard-limit behavior for{' '}
            {target?.label ?? 'the selected scope'}.
          </DialogDescription>
        </DialogHeader>
        <form className="flex flex-col gap-5" onSubmit={editor.save}>
          {isInheritedBudgetSource(target?.source) ? (
            <Alert>
              <AppIcon icon={Alert02Icon} />
              <AlertDescription>{INHERITED_BUDGET_WARNING}</AlertDescription>
            </Alert>
          ) : null}
          <BudgetSettingsFields form={editor.form} setForm={editor.setForm} idPrefix="dialog" />
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={editor.close}>
              Cancel
            </Button>
            <Button type="submit" disabled={editor.isPending}>
              {editor.isPending ? 'Saving...' : 'Save budget'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export function BudgetSettingsFields({
  form,
  setForm,
  idPrefix,
  compact = false,
}: {
  form: BudgetSettingsForm
  setForm: Dispatch<SetStateAction<BudgetSettingsForm>>
  idPrefix: string
  compact?: boolean
}) {
  return (
    <FieldGroup className={cn(compact && 'gap-3')}>
      <div className={cn('grid gap-3', compact ? 'grid-cols-2' : 'sm:grid-cols-2')}>
        <Field>
          <FieldLabel htmlFor={`${idPrefix}-amount`}>Amount (USD)</FieldLabel>
          <Input
            id={`${idPrefix}-amount`}
            inputMode="decimal"
            value={form.amount_usd}
            onChange={(event) => {
              const amount_usd = event.currentTarget.value
              setForm((current) => ({ ...current, amount_usd }))
            }}
            placeholder="100.0000"
            autoComplete="off"
          />
        </Field>
        <Field>
          <FieldLabel htmlFor={`${idPrefix}-cadence`}>Cadence</FieldLabel>
          <Select
            value={form.cadence}
            onValueChange={(value) =>
              setForm((current) => ({
                ...current,
                cadence: value as BudgetSettingsForm['cadence'],
              }))
            }
          >
            <SelectTrigger id={`${idPrefix}-cadence`} className="w-full">
              <SelectValue placeholder="Cadence" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value="daily">Daily</SelectItem>
                <SelectItem value="weekly">Weekly</SelectItem>
                <SelectItem value="monthly">Monthly</SelectItem>
              </SelectGroup>
            </SelectContent>
          </Select>
        </Field>
      </div>
      <Field>
        <FieldLabel htmlFor={`${idPrefix}-timezone`}>Timezone</FieldLabel>
        <Input
          id={`${idPrefix}-timezone`}
          value={form.timezone ?? 'UTC'}
          onChange={(event) => {
            const timezone = event.currentTarget.value
            setForm((current) => ({ ...current, timezone }))
          }}
          placeholder="UTC"
          autoComplete="off"
        />
        {compact ? null : (
          <FieldDescription>Controls when the budget window resets.</FieldDescription>
        )}
      </Field>
      <Field orientation="horizontal">
        <FieldContent>
          <FieldLabel htmlFor={`${idPrefix}-hard-limit`}>Enforce hard limit</FieldLabel>
          {compact ? null : (
            <FieldDescription>Block requests once the budget is exhausted.</FieldDescription>
          )}
        </FieldContent>
        <Switch
          id={`${idPrefix}-hard-limit`}
          checked={form.hard_limit}
          onCheckedChange={(checked) => setForm((current) => ({ ...current, hard_limit: checked }))}
        />
      </Field>
    </FieldGroup>
  )
}
