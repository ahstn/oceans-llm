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
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { canTransferMember } from '@/routes/identity/-teams/team-form'
import type { MembershipChange } from '@/routes/identity/-teams/use-membership-change'
import type { TeamManagementView, TransferTeamMemberInput } from '@/types/api'

/** Confirms removing a member from their team. Owner memberships can't be removed. */
export function RemoveMemberDialog({
  change,
  isPending,
}: {
  change: MembershipChange
  isPending: boolean
}) {
  const { team, user } = change

  return (
    <Dialog open={change.mode === 'remove'} onOpenChange={(open) => !open && change.close()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Remove member</DialogTitle>
          <DialogDescription>
            {team && user
              ? `Remove ${user.name} from ${team.name}. This only changes future membership-derived access.`
              : 'Remove this member from the team.'}
          </DialogDescription>
        </DialogHeader>

        {user && team ? (
          <div className="flex flex-col gap-4">
            <Alert>
              <AlertTitle>Membership removal</AlertTitle>
              <AlertDescription>
                Historical logs, spend, and API-key ownership are not migrated by this action.
              </AlertDescription>
            </Alert>

            {user.team_role === 'owner' ? (
              <Alert>
                <AlertTitle>Owner membership is locked</AlertTitle>
                <AlertDescription>
                  Owner memberships cannot be removed in this slice.
                </AlertDescription>
              </Alert>
            ) : null}

            <DialogFooter>
              <Button type="button" variant="secondary" onClick={change.close}>
                Cancel
              </Button>
              <Button
                type="button"
                variant="destructive"
                onClick={change.removeMember}
                disabled={isPending || user.team_role === 'owner'}
              >
                Remove member
              </Button>
            </DialogFooter>
          </div>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

/** Moves a member to another team with a chosen role. Owner memberships can't be transferred. */
export function TransferMemberDialog({
  change,
  teams,
  isPending,
}: {
  change: MembershipChange
  teams: TeamManagementView[]
  isPending: boolean
}) {
  const { team, user } = change

  return (
    <Dialog open={change.mode === 'transfer'} onOpenChange={(open) => !open && change.close()}>
      <DialogContent className="w-[min(760px,calc(100vw-32px))]">
        <DialogHeader>
          <DialogTitle>Transfer member</DialogTitle>
          <DialogDescription>
            {team && user
              ? `Move ${user.name} out of ${team.name} and into another team. Future access follows the destination membership only.`
              : 'Transfer this member to another team.'}
          </DialogDescription>
        </DialogHeader>

        {user && team ? (
          <form className="flex flex-col gap-5" onSubmit={change.transferMember}>
            {user.team_role === 'owner' ? (
              <Alert>
                <AlertTitle>Owner memberships are locked</AlertTitle>
                <AlertDescription>
                  Owner memberships are not removable or transferable in this slice.
                </AlertDescription>
              </Alert>
            ) : null}

            <TransferDestinationFields change={change} team={team} teams={teams} />

            <Alert>
              <AlertTitle>Transfer is membership-only</AlertTitle>
              <AlertDescription>
                This does not migrate past request logs, spend, budgets, or API-key ownership.
              </AlertDescription>
            </Alert>

            <DialogFooter>
              <Button type="button" variant="secondary" onClick={change.close}>
                Cancel
              </Button>
              <Button
                type="submit"
                disabled={isPending || !canTransferMember(change.transferForm, team, user)}
              >
                Transfer member
              </Button>
            </DialogFooter>
          </form>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

/** Destination team, which leaves out the member's current team, and destination role. */
function TransferDestinationFields({
  change,
  team,
  teams,
}: {
  change: MembershipChange
  team: TeamManagementView
  teams: TeamManagementView[]
}) {
  const { transferForm, setTransferForm } = change

  return (
    <FieldGroup>
      <Field>
        <FieldLabel htmlFor="transfer-destination-team">Destination team</FieldLabel>
        <Select
          value={transferForm.destination_team_id || undefined}
          onValueChange={(value) =>
            setTransferForm((current) => ({
              ...current,
              destination_team_id: value,
            }))
          }
        >
          <SelectTrigger id="transfer-destination-team">
            <SelectValue placeholder="Select destination team" />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              {teams.map((destination) =>
                destination.id === team.id ? null : (
                  <SelectItem key={destination.id} value={destination.id}>
                    {destination.name}
                  </SelectItem>
                ),
              )}
            </SelectGroup>
          </SelectContent>
        </Select>
        <FieldDescription>
          Teams already using the user are omitted. Pick the new ownership boundary explicitly.
        </FieldDescription>
      </Field>

      <Field>
        <FieldLabel htmlFor="transfer-destination-role">Destination role</FieldLabel>
        <Select
          value={transferForm.destination_role}
          onValueChange={(value: TransferTeamMemberInput['destination_role']) =>
            setTransferForm((current) => ({ ...current, destination_role: value }))
          }
        >
          <SelectTrigger id="transfer-destination-role">
            <SelectValue placeholder="Select destination role" />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value="member">Member</SelectItem>
              <SelectItem value="admin">Admin</SelectItem>
            </SelectGroup>
          </SelectContent>
        </Select>
      </Field>
    </FieldGroup>
  )
}
