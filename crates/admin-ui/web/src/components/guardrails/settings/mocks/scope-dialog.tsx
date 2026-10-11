import { useState } from 'react'
import { ArrowDown01Icon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { IconTile } from '@/components/reui/icon-tile'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import type { GuardrailPoliciesView } from '@/types/api'

import { draftChanged, draftFrom } from '../../config-yaml'
import { AreaEditor, YamlPreview } from '../editors'
import {
  AREAS,
  areaSummary,
  scopeKindLabel,
  scopeLabel,
  useGuardrailDrafts,
  type AreaId,
} from '../model'
import { RailButton } from './settings-dialog'

/**
 * Mock B: scopes on the rail, areas as collapsible rows. Reads as "what does this route get?"
 * and keeps every area's state visible at once, at the cost of a longer scroll when expanded.
 */
export function ScopeDialogMock({
  policies,
  open,
  onOpenChange,
}: {
  policies: GuardrailPoliciesView
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const drafts = useGuardrailDrafts(policies)
  const [scopeKey, setScopeKey] = useState(drafts.scopes[0].key)
  const [expanded, setExpanded] = useState<AreaId | null>('destructive')
  const [showYaml, setShowYaml] = useState(false)
  const scope = drafts.scopes.find((item) => item.key === scopeKey) ?? drafts.scopes[0]
  const draft = drafts.drafts[scope.key]

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="grid h-[min(44rem,90vh)] grid-cols-1 gap-0 overflow-hidden p-0 sm:max-w-4xl md:grid-cols-[14rem_minmax(0,1fr)]">
        <nav
          aria-label="Scopes"
          className="bg-muted/40 flex gap-1 overflow-x-auto border-b p-3 pr-12 md:flex-col md:border-r md:border-b-0 md:pr-3"
        >
          {drafts.scopes.map((item) => (
            <RailButton
              key={item.key}
              active={item.key === scope.key}
              onClick={() => setScopeKey(item.key)}
            >
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="truncate">{scopeLabel(item)}</span>
                <span className="text-muted-foreground truncate text-xs">
                  {scopeKindLabel(item)}
                </span>
              </span>
              {draftChanged(drafts.drafts[item.key], draftFrom(item.policy)) ? (
                <Badge variant="warning">Edited</Badge>
              ) : null}
            </RailButton>
          ))}
        </nav>
        <section className="flex min-h-0 min-w-0 flex-col">
          <header className="flex flex-col gap-0.5 border-b p-4 pr-12">
            <DialogTitle>{scopeLabel(scope)}</DialogTitle>
            <DialogDescription>
              {scope.kind === 'global'
                ? 'The default every route and server inherits.'
                : 'Unset fields inherit from the global default.'}
            </DialogDescription>
          </header>
          <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-4">
            {showYaml ? (
              <YamlPreview yaml={drafts.yaml} />
            ) : (
              AREAS.map((area) => {
                const summary = areaSummary(area.id, draft)
                const isOpen = expanded === area.id
                return (
                  <Collapsible
                    key={area.id}
                    open={isOpen}
                    onOpenChange={(next) => setExpanded(next ? area.id : null)}
                    className="rounded-lg border"
                  >
                    <CollapsibleTrigger className="hover:bg-muted/50 flex w-full items-center gap-3 rounded-lg p-3 text-left transition-colors">
                      <IconTile size="sm">
                        <AppIcon icon={area.icon} aria-hidden />
                      </IconTile>
                      <span className="flex min-w-0 flex-1 flex-col">
                        <span className="text-sm font-medium">{area.label}</span>
                        <span className="text-muted-foreground truncate text-xs">
                          {area.description}
                        </span>
                      </span>
                      <Badge variant={summary.tone} className="max-w-48 truncate">
                        {summary.text}
                      </Badge>
                      <AppIcon
                        icon={ArrowDown01Icon}
                        size={16}
                        aria-hidden
                        className={
                          isOpen ? 'rotate-180 transition-transform' : 'transition-transform'
                        }
                      />
                    </CollapsibleTrigger>
                    <CollapsibleContent className="border-t p-4">
                      <AreaEditor
                        area={area.id}
                        draft={draft}
                        policies={policies}
                        onChange={(next) => drafts.update(scope.key, next)}
                      />
                    </CollapsibleContent>
                  </Collapsible>
                )
              })
            )}
          </div>
          <footer className="flex justify-end gap-2 border-t p-3">
            <Button variant="outline" size="sm" onClick={drafts.reset}>
              Reset
            </Button>
            <Button size="sm" onClick={() => setShowYaml(!showYaml)}>
              {showYaml ? 'Back to settings' : 'Review YAML'}
            </Button>
          </footer>
        </section>
      </DialogContent>
    </Dialog>
  )
}
