import { useState } from 'react'

import { AppIcon } from '@/components/icons/app-icon'
import {
  Frame,
  FrameDescription,
  FrameFooter,
  FrameHeader,
  FramePanel,
  FrameTitle,
} from '@/components/reui/frame'
import { IconTile } from '@/components/reui/icon-tile'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import type { GuardrailPoliciesView } from '@/types/api'

import { AreaDialog } from './area-dialog'
import { YamlPreview } from './editors'
import { AREAS, areaSummary, kindLabel, scopeLabel, useGuardrailDrafts, type AreaId } from './model'

/**
 * One card per area showing its state across every scope, each opening its own focused dialog.
 * Integrations reads like a connected-apps list. Nothing is saved: edits only redraft the YAML
 * below, because gateway.yaml stays the only policy authority.
 */
export function GuardrailSettings({ policies }: { policies: GuardrailPoliciesView }) {
  const drafts = useGuardrailDrafts(policies)
  const [open, setOpen] = useState<AreaId | null>(null)
  const [scopeKey, setScopeKey] = useState(drafts.scopes[0].key)
  const manage = (area: AreaId, key = drafts.scopes[0].key) => {
    setScopeKey(key)
    setOpen(area)
  }

  return (
    <div className="flex min-w-0 flex-col gap-6">
      <div className="grid min-w-0 gap-6 lg:grid-cols-3">
        {AREAS.map((area) => (
          <Frame key={area.id} spacing="default" className="min-w-0">
            <FrameHeader className="flex-row items-center gap-3">
              <IconTile size="sm">
                <AppIcon icon={area.icon} aria-hidden />
              </IconTile>
              <div className="flex min-w-0 flex-col gap-0.5">
                <FrameTitle>{area.label}</FrameTitle>
                <FrameDescription className="truncate">{area.description}</FrameDescription>
              </div>
            </FrameHeader>
            <FramePanel className="flex flex-col">
              {area.id === 'integrations' ? (
                <IntegrationRows policies={policies} drafts={drafts} />
              ) : (
                drafts.scopes.map((scope) => {
                  const summary = areaSummary(area.id, drafts.drafts[scope.key])
                  return (
                    <button
                      key={scope.key}
                      type="button"
                      onClick={() => manage(area.id, scope.key)}
                      className="hover:bg-muted dark:hover:bg-muted/50 -mx-2 flex items-center justify-between gap-3 rounded-md px-2 py-2 text-left text-sm transition-colors"
                    >
                      <span className="truncate">{scopeLabel(scope)}</span>
                      <Badge variant={summary.tone} className="max-w-44 truncate">
                        {summary.text}
                      </Badge>
                    </button>
                  )
                })
              )}
            </FramePanel>
            <FrameFooter>
              <Button
                variant="outline"
                size="sm"
                className="w-full"
                onClick={() => manage(area.id)}
              >
                Manage {area.label.toLowerCase()}
              </Button>
            </FrameFooter>
          </Frame>
        ))}
      </div>
      <YamlPreview yaml={drafts.yaml} maxLines={18} />
      <AreaDialog
        area={open}
        scopeKey={scopeKey}
        drafts={drafts}
        policies={policies}
        onScopeChange={setScopeKey}
        onClose={() => setOpen(null)}
      />
    </div>
  )
}

function IntegrationRows({
  policies,
  drafts,
}: {
  policies: GuardrailPoliciesView
  drafts: ReturnType<typeof useGuardrailDrafts>
}) {
  if (policies.managed_checks.length === 0) {
    return <p className="text-muted-foreground py-2 text-sm">No managed checks configured.</p>
  }
  return policies.managed_checks.map((check) => {
    const using = drafts.scopes.filter((scope) => {
      const draft = drafts.drafts[scope.key]
      return draft.enabled && draft.managed_checks.includes(check.name)
    })
    return (
      <div key={check.name} className="flex items-center justify-between gap-3 py-2 text-sm">
        <span className="flex min-w-0 flex-col">
          <span className="truncate font-medium">{check.name}</span>
          <span className="text-muted-foreground truncate text-xs">{kindLabel(check.kind)}</span>
        </span>
        <Badge variant={using.length ? 'success' : 'outline'}>
          {using.length ? `${using.length} ${using.length === 1 ? 'scope' : 'scopes'}` : 'Unused'}
        </Badge>
      </div>
    )
  })
}
