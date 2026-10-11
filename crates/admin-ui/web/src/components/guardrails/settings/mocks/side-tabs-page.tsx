import { useState } from 'react'

import { AppIcon } from '@/components/icons/app-icon'
import {
  Frame,
  FrameDescription,
  FrameHeader,
  FramePanel,
  FrameTitle,
} from '@/components/reui/frame'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import type { GuardrailPoliciesView } from '@/types/api'

import { AreaEditor, ScopeSelect, YamlPreview } from '../editors'
import { AREAS, areaSummary, useGuardrailDrafts, type AreaId } from '../model'
import { RailButton } from './settings-dialog'

/**
 * Mock C: a full settings page. Area rail on the left, the editor in the middle, and the YAML it
 * produces pinned on the right so every toggle shows its config effect immediately.
 */
export function SideTabsPageMock({ policies }: { policies: GuardrailPoliciesView }) {
  const drafts = useGuardrailDrafts(policies)
  const [scopeKey, setScopeKey] = useState(drafts.scopes[0].key)
  const [areaId, setAreaId] = useState<AreaId>('destructive')
  const area = AREAS.find((item) => item.id === areaId) ?? AREAS[0]
  const draft = drafts.drafts[scopeKey]

  return (
    <div className="grid min-w-0 gap-6 md:grid-cols-[13rem_minmax(0,1fr)] xl:grid-cols-[13rem_minmax(0,1fr)_24rem]">
      <nav
        aria-label="Guardrail settings"
        className="flex min-w-0 gap-1 overflow-x-auto md:flex-col"
      >
        {AREAS.map((item) => (
          <RailButton key={item.id} active={item.id === areaId} onClick={() => setAreaId(item.id)}>
            <AppIcon icon={item.icon} size={16} aria-hidden />
            <span className="truncate">{item.label}</span>
          </RailButton>
        ))}
      </nav>
      <Frame spacing="default" className="min-w-0">
        <FrameHeader className="flex-row flex-wrap items-start justify-between gap-3">
          <div className="flex flex-col gap-0.5">
            <FrameTitle>{area.label}</FrameTitle>
            <FrameDescription>{area.description}</FrameDescription>
          </div>
          <ScopeSelect
            scopes={drafts.scopes}
            value={scopeKey}
            onChange={setScopeKey}
            className="w-56"
          />
        </FrameHeader>
        <FramePanel>
          <AreaEditor
            area={area.id}
            draft={draft}
            policies={policies}
            onChange={(next) => drafts.update(scopeKey, next)}
          />
        </FramePanel>
      </Frame>
      <aside className="flex min-w-0 flex-col gap-3 md:col-span-2 xl:sticky xl:top-6 xl:col-span-1 xl:self-start">
        <div className="flex items-center justify-between gap-2">
          <span className="text-sm font-medium">Pending config</span>
          {drafts.changed.length ? (
            <Badge variant="warning">
              {drafts.changed.length} {drafts.changed.length === 1 ? 'scope' : 'scopes'} changed
            </Badge>
          ) : (
            <Badge variant="outline">No changes</Badge>
          )}
        </div>
        <ul className="flex flex-col gap-1 text-xs">
          {AREAS.map((item) => (
            <li key={item.id} className="flex justify-between gap-2">
              <span className="text-muted-foreground">{item.label}</span>
              <span className="truncate">{areaSummary(item.id, draft).text}</span>
            </li>
          ))}
        </ul>
        <YamlPreview yaml={drafts.yaml} maxLines={24} />
        <Button variant="outline" size="sm" onClick={drafts.reset}>
          Reset all
        </Button>
      </aside>
    </div>
  )
}
