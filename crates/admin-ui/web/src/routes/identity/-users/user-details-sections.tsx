import type { Dispatch, SetStateAction } from 'react'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { FieldGroup } from '@/components/ui/field'
import { EntityTagsField } from '@/routes/identity/-entity-tags'
import type { UpdateUserForm } from '@/routes/identity/-user-form'
import { MembershipFields } from '@/routes/identity/-user-form-fields'
import type { IdentityUsersPayload, UserView } from '@/types/api'

export function UserOverviewSection({ user }: { user: UserView }) {
  return (
    <div>
      <h3 className="text-foreground text-sm font-semibold">Profile</h3>
      <dl className="mt-5 grid gap-x-8 gap-y-6 text-sm sm:grid-cols-2 lg:grid-cols-3">
        <UserDetailRow label="Global role" value={formatRole(user.global_role)} />
        <UserDetailRow label="Team" value={user.team_name ?? 'No team'} />
        <UserDetailRow label="Team role" value={user.team_role ?? '—'} />
        <UserDetailRow label="Auth method" value={user.auth_mode} />
        <UserDetailRow
          label="Request logging"
          value={user.request_logging_enabled ? 'Enabled' : 'Disabled'}
        />
        <UserDetailRow label="Status" value={user.status} />
      </dl>
    </div>
  )
}

export function UserConfigurationSection({
  user,
  form,
  setForm,
  teams,
}: {
  user: UserView
  form: UpdateUserForm
  setForm: Dispatch<SetStateAction<UpdateUserForm>>
  teams: IdentityUsersPayload['teams']
}) {
  return (
    <div className="flex flex-col gap-4">
      {user.team_role === 'owner' ? (
        <Alert>
          <AlertTitle>Owner membership is locked</AlertTitle>
          <AlertDescription>
            This user is an owner on their current team. In this slice, owner memberships cannot be
            moved or changed through the admin UI.
          </AlertDescription>
        </Alert>
      ) : null}

      <FieldGroup>
        <MembershipFields
          idPrefix="manage-"
          form={form}
          setForm={setForm}
          teams={teams}
          locked={user.team_role === 'owner'}
        />
      </FieldGroup>

      <EntityTagsField
        label="Tags"
        tags={form.tags}
        onChange={(tags) => setForm((current) => ({ ...current, tags }))}
      />
    </div>
  )
}

export function UserUsageSection() {
  return (
    <div className="grid gap-4 md:grid-cols-3">
      <section className="border-border rounded-lg border p-4">
        <p className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
          Favourite model
        </p>
        <p className="text-foreground mt-2 text-lg font-semibold">Not enough data</p>
        <p className="text-subtle-foreground mt-1 text-sm">
          Wire this to model-level usage once the backend exposes per-user aggregates.
        </p>
      </section>
      <section className="border-border rounded-lg border p-4">
        <p className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
          Weekly usage
        </p>
        <p className="text-foreground mt-2 text-lg font-semibold">Pending</p>
        <p className="text-subtle-foreground mt-1 text-sm">
          Planned: spend, tokens, and request counts over the last seven days.
        </p>
      </section>
      <section className="border-border rounded-lg border p-4">
        <p className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
          Recent activity
        </p>
        <p className="text-foreground mt-2 text-lg font-semibold">Pending</p>
        <p className="text-subtle-foreground mt-1 text-sm">
          Planned: latest requests and policy outcomes for this user.
        </p>
      </section>
    </div>
  )
}

function UserDetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
        {label}
      </dt>
      <dd className="text-subtle-foreground mt-1">{value}</dd>
    </div>
  )
}

function formatRole(role: string) {
  return role
    .split('_')
    .filter(Boolean)
    .map((part) => part[0]?.toUpperCase() + part.slice(1))
    .join(' ')
}
