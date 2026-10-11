import { createFileRoute, Link } from '@tanstack/react-router'
import { Activity01Icon } from '@hugeicons/core-free-icons'

import { GuardrailSettings } from '@/components/guardrails/settings/settings-page'
import { AppIcon } from '@/components/icons/app-icon'
import { PageHeader } from '@/components/layout/page-header'
import { Button } from '@/components/ui/button'
import { getGuardrailPolicyView } from '@/server/admin-data.functions'

export const Route = createFileRoute('/guardrails')({
  loader: async () => ({ policies: (await getGuardrailPolicyView()).data }),
  component: GuardrailSettingsPage,
})

export function GuardrailSettingsPage() {
  const { policies } = Route.useLoaderData()
  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Control Plane"
        title="Guardrail Settings"
        description="Draft destructive-action, secret-redaction and integration settings as gateway.yaml. Nothing is saved here."
        actions={
          <Button variant="outline" size="sm" asChild>
            <Link to="/observability/guardrails">
              <AppIcon icon={Activity01Icon} data-icon="inline-start" aria-hidden />
              Activity
            </Link>
          </Button>
        }
      />
      <GuardrailSettings policies={policies} />
    </div>
  )
}
