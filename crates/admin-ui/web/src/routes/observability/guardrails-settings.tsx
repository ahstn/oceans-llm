import { useState } from 'react'
import { createFileRoute, Link, useRouter } from '@tanstack/react-router'

import { AreaCardsPageMock } from '@/components/guardrails/settings/mocks/area-cards-page'
import { ScopeDialogMock } from '@/components/guardrails/settings/mocks/scope-dialog'
import { ScopeMatrixPageMock } from '@/components/guardrails/settings/mocks/scope-matrix-page'
import { SettingsDialogMock } from '@/components/guardrails/settings/mocks/settings-dialog'
import { SideTabsPageMock } from '@/components/guardrails/settings/mocks/side-tabs-page'
import { PageHeader } from '@/components/layout/page-header'
import { Button } from '@/components/ui/button'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from '@/components/ui/empty'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { getGuardrailPolicyView } from '@/server/admin-data.functions'

/**
 * Design mocks for guardrail configuration. Each mock gives destructive actions, secret redaction
 * and integrations their own surface and drafts gateway.yaml; none of them saves anything. The
 * chosen one replaces the Configure sheet on the guardrails page and this route goes.
 */
const MOCKS = {
  'settings-dialog': { label: 'Settings dialog', kind: 'dialog' },
  'scope-dialog': { label: 'Scope dialog', kind: 'dialog' },
  'side-tabs': { label: 'Side tabs page', kind: 'page' },
  'area-cards': { label: 'Area cards page', kind: 'page' },
  'scope-matrix': { label: 'Scope matrix page', kind: 'page' },
} as const

type MockId = keyof typeof MOCKS

export const Route = createFileRoute('/observability/guardrails-settings')({
  validateSearch: (search: Record<string, unknown>): { mock?: MockId } =>
    typeof search.mock === 'string' && search.mock in MOCKS ? { mock: search.mock as MockId } : {},
  loader: async () => ({ policies: (await getGuardrailPolicyView()).data }),
  component: GuardrailSettingsLabPage,
})

export function GuardrailSettingsLabPage() {
  const { policies } = Route.useLoaderData()
  const { mock = 'settings-dialog' } = Route.useSearch()
  const router = useRouter()
  const [dialogOpen, setDialogOpen] = useState(true)
  const isDialog = MOCKS[mock].kind === 'dialog'

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Observability"
        title="Guardrail settings"
        description="Configuration mocks. Each drafts gateway.yaml; nothing is saved."
        actions={
          <div className="flex flex-wrap items-center gap-2">
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              aria-label="Mock"
              value={mock}
              onValueChange={(value) => {
                if (!value) return
                setDialogOpen(true)
                void router.navigate({
                  to: '/observability/guardrails-settings',
                  search: { mock: value as MockId },
                })
              }}
            >
              {(Object.keys(MOCKS) as MockId[]).map((id) => (
                <ToggleGroupItem key={id} value={id}>
                  {MOCKS[id].label}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
            <Button variant="outline" size="sm" asChild>
              <Link to="/observability/guardrails">Guardrails</Link>
            </Button>
          </div>
        }
      />
      {isDialog ? (
        <Empty className="border">
          <EmptyHeader>
            <EmptyTitle>{MOCKS[mock].label}</EmptyTitle>
            <EmptyDescription>Opens from Configure on the guardrails page.</EmptyDescription>
          </EmptyHeader>
          <EmptyContent>
            <Button onClick={() => setDialogOpen(true)}>Open settings</Button>
          </EmptyContent>
        </Empty>
      ) : null}
      {mock === 'settings-dialog' ? (
        <SettingsDialogMock policies={policies} open={dialogOpen} onOpenChange={setDialogOpen} />
      ) : null}
      {mock === 'scope-dialog' ? (
        <ScopeDialogMock policies={policies} open={dialogOpen} onOpenChange={setDialogOpen} />
      ) : null}
      {mock === 'side-tabs' ? <SideTabsPageMock policies={policies} /> : null}
      {mock === 'area-cards' ? <AreaCardsPageMock policies={policies} /> : null}
      {mock === 'scope-matrix' ? <ScopeMatrixPageMock policies={policies} /> : null}
    </div>
  )
}
