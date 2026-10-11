import { useState } from 'react'

import { AppIcon } from '@/components/icons/app-icon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { Separator } from '@/components/ui/separator'
import type { GuardrailPoliciesView } from '@/types/api'

import { AreaEditor, ScopeSelect, YamlPreview } from '../editors'
import { AREAS, areaSummary, useGuardrailDrafts, type AreaId } from '../model'

type View = AreaId | 'review'

/**
 * Mock A: one settings dialog with an area rail. Scope is picked once at the top of the rail and
 * each area gets the full content pane; "Review" shows the YAML for every edited scope.
 */
export function SettingsDialogMock({
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
  const [view, setView] = useState<View>('destructive')
  const draft = drafts.drafts[scopeKey]
  const area = AREAS.find((item) => item.id === view)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="grid h-[min(44rem,90vh)] grid-cols-1 gap-0 overflow-hidden p-0 sm:max-w-5xl md:grid-cols-[15rem_minmax(0,1fr)]">
        <nav
          aria-label="Guardrail settings"
          className="bg-muted/40 flex flex-col gap-3 border-b p-3 pr-12 md:border-r md:border-b-0 md:pr-3"
        >
          <ScopeSelect
            scopes={drafts.scopes}
            value={scopeKey}
            onChange={setScopeKey}
            className="w-full"
          />
          <div className="flex gap-1 overflow-x-auto md:flex-col">
            {AREAS.map((item) => (
              <RailButton key={item.id} active={view === item.id} onClick={() => setView(item.id)}>
                <AppIcon icon={item.icon} size={16} aria-hidden />
                <span className="flex min-w-0 flex-col">
                  <span className="truncate">{item.label}</span>
                  <span className="text-muted-foreground truncate text-xs">
                    {areaSummary(item.id, draft).text}
                  </span>
                </span>
              </RailButton>
            ))}
            <Separator className="my-1 hidden md:block" />
            <RailButton active={view === 'review'} onClick={() => setView('review')}>
              <span className="flex-1">Review YAML</span>
              {drafts.changed.length ? (
                <Badge variant="warning">{drafts.changed.length}</Badge>
              ) : null}
            </RailButton>
          </div>
        </nav>
        <section className="flex min-h-0 min-w-0 flex-col">
          <header className="flex flex-col gap-0.5 border-b p-4 pr-12">
            <DialogTitle>{area?.label ?? 'Review changes'}</DialogTitle>
            <DialogDescription>
              {area?.description ?? 'Copy into gateway.yaml and redeploy. Nothing is saved here.'}
            </DialogDescription>
          </header>
          <div className="min-h-0 flex-1 overflow-y-auto p-4">
            {area ? (
              <AreaEditor
                area={area.id}
                draft={draft}
                policies={policies}
                onChange={(next) => drafts.update(scopeKey, next)}
              />
            ) : (
              <YamlPreview yaml={drafts.yaml} />
            )}
          </div>
          <footer className="flex items-center justify-between gap-2 border-t p-3">
            <span className="text-muted-foreground text-xs">
              {drafts.changed.length
                ? `${drafts.changed.length} ${drafts.changed.length === 1 ? 'scope' : 'scopes'} changed`
                : 'No changes'}
            </span>
            <div className="flex gap-2">
              <Button variant="outline" size="sm" onClick={drafts.reset}>
                Reset
              </Button>
              <Button size="sm" onClick={() => setView('review')}>
                Review YAML
              </Button>
            </div>
          </footer>
        </section>
      </DialogContent>
    </Dialog>
  )
}

export function RailButton({
  active,
  onClick,
  children,
}: {
  active: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      type="button"
      aria-current={active ? 'page' : undefined}
      onClick={onClick}
      className="hover:bg-muted aria-[current=page]:bg-muted dark:hover:bg-muted/50 dark:aria-[current=page]:bg-muted/50 flex min-w-40 items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors md:min-w-0"
    >
      {children}
    </button>
  )
}
