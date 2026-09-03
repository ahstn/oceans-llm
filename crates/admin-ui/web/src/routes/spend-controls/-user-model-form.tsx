import { useState, type FormEvent } from 'react'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import type { BudgetScopeRequest, ModelView, SpendBudgetUserView } from '@/types/api'

import { BudgetSettingsFields, type BudgetEditor } from './-budget-editor'
import { initialBudgetSettings, type BudgetSettingsForm } from './-budget-model'

type UserModelDraft = {
  userId: string
  selectorKind: 'model_id' | 'upstream_model'
  selectorValue: string
  settings: BudgetSettingsForm
}

function initialDraft(users: SpendBudgetUserView[], models: ModelView[]): UserModelDraft {
  return {
    userId: users[0]?.user_id ?? '',
    selectorKind: 'model_id',
    selectorValue: models[0]?.model_id ?? '',
    settings: initialBudgetSettings,
  }
}

// A linear controlled form; the selector kind swaps one field and the rest is shared.
// oxlint-disable-next-line eslint/max-lines-per-function
export function UserModelBudgetForm({
  users,
  models,
  editor,
}: {
  users: SpendBudgetUserView[]
  models: ModelView[]
  editor: BudgetEditor
}) {
  const [draft, setDraft] = useState<UserModelDraft>(() => initialDraft(users, models))

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const selectorValue = draft.selectorValue.trim()
    if (!draft.userId || !selectorValue) {
      toast.error('Select a user and model scope before saving')
      return
    }
    const scope: BudgetScopeRequest =
      draft.selectorKind === 'model_id'
        ? { kind: 'user_model', user_id: draft.userId, model_id: selectorValue }
        : { kind: 'user_model', user_id: draft.userId, upstream_model: selectorValue }
    editor.upsert(scope, draft.settings, 'User model budget created', () =>
      setDraft(initialDraft(users, models)),
    )
  }

  return (
    <form className="flex flex-col gap-4" onSubmit={submit}>
      <FieldGroup className="gap-3">
        <Field>
          <FieldLabel htmlFor="user-model-user">User</FieldLabel>
          <Select
            value={draft.userId}
            onValueChange={(value) => setDraft((current) => ({ ...current, userId: value }))}
          >
            <SelectTrigger id="user-model-user" className="w-full">
              <SelectValue placeholder="User" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {users.map((user) => (
                  <SelectItem key={user.user_id} value={user.user_id}>
                    {user.name}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
        </Field>
        <div className="grid gap-3 sm:grid-cols-[160px_minmax(0,1fr)]">
          <Field>
            <FieldLabel htmlFor="user-model-kind">Scope</FieldLabel>
            <Select
              value={draft.selectorKind}
              onValueChange={(value) =>
                setDraft((current) => ({
                  ...current,
                  selectorKind: value as UserModelDraft['selectorKind'],
                  selectorValue: value === 'model_id' ? (models[0]?.model_id ?? '') : '',
                }))
              }
            >
              <SelectTrigger id="user-model-kind" className="w-full">
                <SelectValue placeholder="Scope type" />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectItem value="model_id">Model id</SelectItem>
                  <SelectItem value="upstream_model">Upstream model</SelectItem>
                </SelectGroup>
              </SelectContent>
            </Select>
          </Field>
          <Field>
            <FieldLabel htmlFor="user-model-value">
              {draft.selectorKind === 'model_id' ? 'Model' : 'Upstream model'}
            </FieldLabel>
            {draft.selectorKind === 'model_id' ? (
              <Select
                value={draft.selectorValue}
                onValueChange={(value) =>
                  setDraft((current) => ({ ...current, selectorValue: value }))
                }
              >
                <SelectTrigger id="user-model-value" className="w-full">
                  <SelectValue placeholder="Model" />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {models.map((model) => (
                      <SelectItem key={model.model_id} value={model.model_id}>
                        {model.id}
                        {model.resolved_model_key !== model.id ? (
                          <span className="text-muted-foreground">
                            {' '}
                            · {model.resolved_model_key}
                          </span>
                        ) : null}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            ) : (
              <Input
                id="user-model-value"
                value={draft.selectorValue}
                onChange={(event) => {
                  const selectorValue = event.currentTarget.value
                  setDraft((current) => ({ ...current, selectorValue }))
                }}
                placeholder="provider/model"
                autoComplete="off"
              />
            )}
          </Field>
        </div>
      </FieldGroup>
      <BudgetSettingsFields
        form={draft.settings}
        setForm={(update) =>
          setDraft((current) => ({
            ...current,
            settings: typeof update === 'function' ? update(current.settings) : update,
          }))
        }
        idPrefix="user-model"
        compact
      />
      <div className="flex justify-end">
        <Button type="submit" disabled={editor.isPending}>
          Add model budget
        </Button>
      </div>
    </form>
  )
}
