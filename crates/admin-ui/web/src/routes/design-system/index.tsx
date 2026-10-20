import { createFileRoute } from '@tanstack/react-router'

import { PageHeader } from '@/components/layout/page-header'
import { Button } from '@/components/ui/button'

import { ActionsSection, InputsSection } from './-components/actions-inputs'
import { DataDisplaySection, NavigationSection } from './-components/data-display'
import { FeedbackSection } from './-components/feedback'
import { FoundationsSection } from './-components/foundations'
import { OverlaysSection } from './-components/overlays'
import { GuidelinesSection, RecipesSection } from './-components/patterns'

export const Route = createFileRoute('/design-system/')({
  component: DesignSystemPage,
})

const sections = [
  { id: 'foundations', label: 'Foundations' },
  { id: 'actions', label: 'Actions' },
  { id: 'inputs', label: 'Inputs' },
  { id: 'data-display', label: 'Data display' },
  { id: 'navigation', label: 'Navigation' },
  { id: 'feedback', label: 'Feedback' },
  { id: 'overlays', label: 'Overlays' },
  { id: 'guidelines', label: 'Do and don’t' },
  { id: 'recipes', label: 'Recipes' },
] as const

/** Living reference for the admin UI tokens and components. DESIGN.md is the written source. */
function DesignSystemPage() {
  return (
    <div className="flex flex-col gap-8">
      <PageHeader
        section="Reference"
        title="Design system"
        description="Tokens, components and patterns for the Oceans Gateway admin UI: Reef surfaces from the account-ready art, Tokyo Night lilac for actions and Tokyo Night colours for data. Every swatch and example renders from the live theme."
      />
      <nav aria-label="Design system sections" className="flex flex-wrap gap-1">
        {sections.map((section) => (
          <Button key={section.id} variant="ghost" size="sm" asChild>
            <a href={`#${section.id}`}>{section.label}</a>
          </Button>
        ))}
      </nav>
      <FoundationsSection />
      <ActionsSection />
      <InputsSection />
      <DataDisplaySection />
      <NavigationSection />
      <FeedbackSection />
      <OverlaysSection />
      <GuidelinesSection />
      <RecipesSection />
    </div>
  )
}
