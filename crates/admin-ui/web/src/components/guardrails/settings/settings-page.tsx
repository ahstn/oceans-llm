import { useState } from 'react'

import { AppIcon } from '@/components/icons/app-icon'
import {
  Frame,
  FrameDescription,
  FrameFooter,
  FrameHeader,
  FrameTitle,
} from '@/components/reui/frame'
import { IconTile } from '@/components/reui/icon-tile'
import { Button } from '@/components/ui/button'
import type { GuardrailPoliciesView } from '@/types/api'

import { AreaDialog } from './area-dialog'
import { YamlPreview } from './editors'
import { AREAS, useGuardrailDrafts, type AreaId } from './model'

/**
 * One card per area, each opening its own focused dialog. Nothing is saved: edits only redraft
 * the YAML below, because gateway.yaml stays the only policy authority.
 */
export function GuardrailSettings({ policies }: { policies: GuardrailPoliciesView }) {
  const drafts = useGuardrailDrafts(policies)
  const [open, setOpen] = useState<AreaId | null>(null)
  const [scopeKey, setScopeKey] = useState(drafts.scopes[0].key)
  const manage = (area: AreaId) => {
    setScopeKey(drafts.scopes[0].key)
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
