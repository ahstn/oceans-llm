import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import type { GuardrailPoliciesView } from '@/types/api'

import { AreaEditor, ScopeSelect } from './editors'
import { AREAS, type AreaId, type GuardrailDrafts } from './model'

/** One focused dialog per area, opened from that area's card on the settings page. */
export function AreaDialog({
  area,
  scopeKey,
  drafts,
  policies,
  onScopeChange,
  onClose,
}: {
  area: AreaId | null
  scopeKey: string
  drafts: GuardrailDrafts
  policies: GuardrailPoliciesView
  onScopeChange: (key: string) => void
  onClose: () => void
}) {
  const meta = AREAS.find((item) => item.id === area)
  return (
    <Dialog open={area !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="flex max-h-[85vh] flex-col sm:max-w-xl">
        {meta ? (
          <>
            <DialogHeader>
              <DialogTitle>{meta.label}</DialogTitle>
              <DialogDescription>{meta.description}</DialogDescription>
            </DialogHeader>
            <ScopeSelect
              scopes={drafts.scopes}
              value={scopeKey}
              onChange={onScopeChange}
              className="w-full"
            />
            <div className="-mx-4 min-h-0 flex-1 overflow-y-auto px-4">
              <AreaEditor
                area={meta.id}
                draft={drafts.drafts[scopeKey]}
                scopeKind={drafts.scopes.find((scope) => scope.key === scopeKey)?.kind ?? 'global'}
                policies={policies}
                onChange={(next) => drafts.update(scopeKey, next)}
              />
            </div>
            <DialogFooter>
              <Button onClick={onClose}>Done</Button>
            </DialogFooter>
          </>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}
