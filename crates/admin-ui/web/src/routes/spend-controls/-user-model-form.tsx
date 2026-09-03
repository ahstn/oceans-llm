import type { Dispatch, FormEvent, SetStateAction } from 'react'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import type { ModelView, SpendBudgetUserView } from '@/types/api'

import type { BudgetSettingsForm, UserModelDraft } from './-utils'

type DraftSetter = Dispatch<SetStateAction<UserModelDraft>>

export function UserModelBudgetForm({
  users,
  models,
  draft,
  setDraft,
  isPending,
  onSubmit,
}: {
  users: SpendBudgetUserView[]
  models: ModelView[]
  draft: UserModelDraft
  setDraft: DraftSetter
  isPending: boolean
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
}) {
  function updateSettings(patch: Partial<BudgetSettingsForm>) {
    setDraft((current) => ({ ...current, settings: { ...current.settings, ...patch } }))
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Add User Model Budget</CardTitle>
        <CardDescription>
          Create a budget for one user and either a managed model id or an upstream model name.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <form
          className="grid gap-3 lg:grid-cols-[220px_190px_minmax(0,1fr)_140px_130px_120px_100px_100px]"
          onSubmit={onSubmit}
        >
          <Select
            value={draft.userId}
            onValueChange={(value) => setDraft((current) => ({ ...current, userId: value }))}
          >
            <SelectTrigger aria-label="User">
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
          <Select
            value={draft.selectorKind}
            onValueChange={(value) =>
              setDraft((current) => ({
                ...current,
                selectorKind: value as UserModelDraft['selectorKind'],
                selectorValue: '',
              }))
            }
          >
            <SelectTrigger aria-label="Scope type">
              <SelectValue placeholder="Scope type" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value="model_id">Model id</SelectItem>
                <SelectItem value="upstream_model">Upstream model</SelectItem>
              </SelectGroup>
            </SelectContent>
          </Select>
          <ModelSelector models={models} draft={draft} setDraft={setDraft} />
          <Input
            aria-label="Amount (USD)"
            value={draft.settings.amount_usd}
            onChange={(event) => updateSettings({ amount_usd: event.currentTarget.value })}
            placeholder="100.0000"
            autoComplete="off"
          />
          <Select
            value={draft.settings.cadence}
            onValueChange={(value) => updateSettings({ cadence: value })}
          >
            <SelectTrigger aria-label="Cadence">
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
          <Input
            aria-label="Timezone"
            value={draft.settings.timezone ?? 'UTC'}
            onChange={(event) => updateSettings({ timezone: event.currentTarget.value })}
            placeholder="UTC"
            autoComplete="off"
          />
          <label className="flex min-h-10 items-center gap-2 text-sm text-[var(--color-text)]">
            <input
              type="checkbox"
              checked={draft.settings.hard_limit}
              onChange={(event) => updateSettings({ hard_limit: event.currentTarget.checked })}
            />
            Hard
          </label>
          <Button type="submit" disabled={isPending}>
            Add
          </Button>
        </form>
      </CardContent>
    </Card>
  )
}

function ModelSelector({
  models,
  draft,
  setDraft,
}: {
  models: ModelView[]
  draft: UserModelDraft
  setDraft: DraftSetter
}) {
  if (draft.selectorKind === 'model_id') {
    return (
      <Select
        value={draft.selectorValue}
        onValueChange={(value) => setDraft((current) => ({ ...current, selectorValue: value }))}
      >
        <SelectTrigger aria-label="Model">
          <SelectValue placeholder="Model" />
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            {models.map((model) => (
              <SelectItem key={model.model_id} value={model.model_id}>
                {model.resolved_model_key}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    )
  }
  return (
    <Input
      aria-label="Upstream model"
      value={draft.selectorValue}
      onChange={({ currentTarget: { value } }) =>
        setDraft((current) => ({ ...current, selectorValue: value }))
      }
      placeholder="provider/model"
      autoComplete="off"
    />
  )
}
