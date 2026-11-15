import {
  Analytics01Icon,
  FlowIcon,
  GitPullRequestIcon,
  Layers01Icon,
  Robot01Icon,
  Rocket01Icon,
  Shield01Icon,
  UserIcon,
} from '@hugeicons/core-free-icons'
import type { IconSvgElement } from '@hugeicons/react'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import { lazy, Suspense, type ComponentType } from 'react'

import { AppIcon } from '@/components/icons/app-icon'
import { PageHeader } from '@/components/layout/page-header'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

import { CONCEPT_WINDOWS, type ConceptWindow } from './-attribution-concepts/metrics'
import { READINESS, ReadinessBadge, type Readiness } from './-attribution-concepts/parts'

type ConceptProps = { days: ConceptWindow }

type Concept = {
  id: string
  label: string
  icon: IconSvgElement
  question: string
  component: ComponentType<ConceptProps>
}

// Each concept loads on its own, so Recharts and the sample generator stay out of the route chunk.
const CONCEPTS: Concept[] = [
  {
    id: 'impact',
    label: 'Impact',
    icon: Analytics01Icon,
    question: 'What did AI deliver, and what did it cost?',
    component: lazy(() => import('./-attribution-concepts/concept-impact')),
  },
  {
    id: 'outcomes',
    label: 'Session outcomes',
    icon: FlowIcon,
    question: 'Where does agent work, and agent spend, end up?',
    component: lazy(() => import('./-attribution-concepts/concept-outcomes')),
  },
  {
    id: 'delivery',
    label: 'Delivery',
    icon: Rocket01Icon,
    question: 'How does AI change lead time, batch size and stability (DORA)?',
    component: lazy(() => import('./-attribution-concepts/concept-delivery')),
  },
  {
    id: 'pull-requests',
    label: 'Pull requests',
    icon: GitPullRequestIcon,
    question: 'What went into each PR: AI share, sessions, cost and time to merge?',
    component: lazy(() => import('./-attribution-concepts/concept-pull-requests')),
  },
  {
    id: 'repositories',
    label: 'Repositories',
    icon: Layers01Icon,
    question: 'Which codebases are agent-ready, and which need harness work?',
    component: lazy(() => import('./-attribution-concepts/concept-repositories')),
  },
  {
    id: 'agents',
    label: 'Agents & models',
    icon: Robot01Icon,
    question: 'Which harness and model pairs turn spend into shipped code?',
    component: lazy(() => import('./-attribution-concepts/concept-agents')),
  },
  {
    id: 'durability',
    label: 'Durability',
    icon: Shield01Icon,
    question: 'Does AI code last, and what would we need to collect to know?',
    component: lazy(() => import('./-attribution-concepts/concept-durability')),
  },
  {
    id: 'my-work',
    label: 'My work',
    icon: UserIcon,
    question: 'How is my own work with agents changing over time?',
    component: lazy(() => import('./-attribution-concepts/concept-my-work')),
  },
]

type ConceptSearch = { tab: string; days: ConceptWindow }

export const Route = createFileRoute('/observability/attribution_/concepts')({
  validateSearch: (search: Record<string, unknown>): ConceptSearch => {
    const tab = CONCEPTS.some((concept) => concept.id === search.tab)
      ? String(search.tab)
      : 'impact'
    const days = CONCEPT_WINDOWS.find((window) => window === Number(search.days)) ?? 30
    return { tab, days }
  },
  component: AttributionConceptsPage,
})

// Design exploration for the Attribution page: eight candidate tabs on sample data, each widget
// labelled with how close it is to data Oceans already has.
export function AttributionConceptsPage() {
  const search = Route.useSearch()
  const navigate = useNavigate({ from: Route.fullPath })
  const concept = CONCEPTS.find((entry) => entry.id === search.tab) ?? CONCEPTS[0]
  const Concept = concept.component

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Observability · Concepts"
        title="Attribution"
        description={concept.question}
        actions={
          <ToggleGroup
            type="single"
            variant="outline"
            value={String(search.days)}
            onValueChange={(next) => {
              if (next)
                void navigate({ search: { tab: search.tab, days: Number(next) as ConceptWindow } })
            }}
            aria-label="Report window"
          >
            {CONCEPT_WINDOWS.map((window) => (
              <ToggleGroupItem key={window} value={String(window)}>
                {window}d
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        }
      />

      <nav
        aria-label="Attribution concepts"
        className="flex flex-wrap items-center gap-2 border-b pb-4"
      >
        {CONCEPTS.map((entry, index) => (
          <Button key={entry.id} asChild variant={entry.id === concept.id ? 'secondary' : 'ghost'}>
            <Link
              to="/observability/attribution/concepts"
              search={{ tab: entry.id, days: search.days }}
              aria-current={entry.id === concept.id ? 'page' : undefined}
            >
              <AppIcon icon={entry.icon} size={16} aria-hidden data-icon="inline-start" />
              <span className="text-muted-foreground tabular-nums">{index + 1}</span>
              {entry.label}
            </Link>
          </Button>
        ))}
      </nav>

      <Alert>
        <AlertTitle>Concept mockup on sample data</AlertTitle>
        <AlertDescription className="flex flex-col gap-2">
          <span>
            A fictional six-person team over 200 days, generated in the browser. Every section
            carries a badge that says how far it is from real data:
          </span>
          <span className="flex flex-wrap gap-x-4 gap-y-2">
            {(Object.keys(READINESS) as Readiness[]).map((level) => (
              <span key={level} className="inline-flex items-center gap-1.5 text-xs">
                <ReadinessBadge level={level} />
                <span className="text-muted-foreground">{READINESS[level].detail}</span>
              </span>
            ))}
          </span>
        </AlertDescription>
      </Alert>

      <Suspense fallback={<ConceptSkeleton />}>
        <Concept key={concept.id} days={search.days} />
      </Suspense>
    </div>
  )
}

function ConceptSkeleton() {
  return (
    <div className="flex flex-col gap-6" aria-busy>
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        {['a', 'b', 'c', 'd'].map((key) => (
          <Skeleton key={key} className="h-32 rounded-xl" />
        ))}
      </div>
      <Skeleton className="h-80 rounded-xl" />
    </div>
  )
}
