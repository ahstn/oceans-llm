import { HugeiconsIcon } from '@hugeicons/react'
import { Add01Icon, Key01Icon } from '@hugeicons/core-free-icons'

import authHeroWave from '@/assets/auth-hero-wave.png'
import oceansLogo from '@/assets/oceans-logo-rounded-square.png'
import { PageHeader } from '@/components/layout/page-header'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Progress } from '@/components/ui/progress'
import { Separator } from '@/components/ui/separator'

import { DoDont, ShowcaseBlock, ShowcaseSection } from './showcase'

export function GuidelinesSection() {
  return (
    <ShowcaseSection
      id="guidelines"
      title="Do and don't"
      description="The rules most often broken in review, each shown both ways."
    >
      <ShowcaseBlock
        title="Rules"
        description="From DESIGN.md. The page is the reference; DESIGN.md is the source."
      >
        <DoDont
          rule="One primary action per group."
          good={
            <>
              <Button>Create key</Button>
              <Button variant="outline">Import</Button>
            </>
          }
          bad={
            <>
              <Button>Create key</Button>
              <Button>Import</Button>
            </>
          }
        />
        <Separator />
        <DoDont
          rule="Status colours describe state, not selection or categories."
          good={
            <>
              <Badge variant="success">Deployed</Badge>
              <Badge variant="secondary">Selected</Badge>
            </>
          }
          bad={
            <>
              <Badge variant="success">Selected</Badge>
              <Badge variant="success">Weekly</Badge>
            </>
          }
        />
        <Separator />
        <DoDont
          rule="Monospace is for identifiers, not labels or prose."
          good={
            <p className="text-sm">
              Key <span className="font-mono">key_01JB7Q4M</span> belongs to Platform.
            </p>
          }
          bad={<p className="font-mono text-sm">Key belongs to Platform.</p>}
        />
        <Separator />
        <DoDont
          rule="Lilac marks what you can act on; data colours mark data. Don't swap them."
          good={
            <>
              <Button size="sm">Create key</Button>
              <span className="flex items-center gap-1.5 text-sm">
                <span className="bg-chart-1 size-2.5 rounded-xs" />
                Tokens
              </span>
            </>
          }
          bad={
            <>
              <Button size="sm" className="bg-chart-2 hover:bg-chart-2/80">
                Create key
              </Button>
              <span className="flex items-center gap-1.5 text-sm">
                <span className="bg-primary size-2.5 rounded-xs" />
                Tokens
              </span>
            </>
          }
        />
      </ShowcaseBlock>
    </ShowcaseSection>
  )
}

export function RecipesSection() {
  return (
    <ShowcaseSection
      id="recipes"
      title="Recipes"
      description="Components composed the way product pages use them."
    >
      <div className="grid gap-4 lg:grid-cols-2">
        <section
          aria-label="Auth hero panel"
          className="relative flex min-h-80 overflow-hidden rounded-xl border"
        >
          <img src={authHeroWave} alt="" className="absolute inset-0 size-full object-cover" />
          <div className="relative flex size-full flex-col justify-between p-6">
            <div className="flex items-center gap-3">
              <img src={oceansLogo} alt="" className="size-10 rounded-lg" />
              <span className="font-heading text-lg font-medium">Oceans Gateway</span>
            </div>
            <p className="type-display max-w-xs text-balance">Secure the gateway.</p>
          </div>
        </section>

        <Card>
          <CardHeader>
            <CardTitle>Budget</CardTitle>
            <CardDescription>Platform team, resets Monday 00:00 UTC.</CardDescription>
            <CardAction>
              <Badge variant="outline">Weekly</Badge>
            </CardAction>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            <div className="flex items-baseline justify-between gap-2">
              <span className="text-2xl font-semibold tabular-nums">$186.40</span>
              <span className="text-muted-foreground text-sm tabular-nums">of $250.00</span>
            </div>
            <Progress value={74.5} aria-label="Weekly budget used" />
            <p className="text-muted-foreground text-sm">
              $63.60 left. Requests are blocked when it runs out.
            </p>
          </CardContent>
          <CardFooter className="gap-2">
            <Button variant="outline" size="sm">
              Edit budget
            </Button>
          </CardFooter>
        </Card>
      </div>

      <ShowcaseBlock
        title="Example screen"
        description="A list page: header with one primary action, stat cards, then the rows."
      >
        <div className="bg-background flex flex-col gap-6 rounded-xl border p-4 sm:p-6">
          <PageHeader
            section="Personal"
            title="API Keys"
            description="Keys you own or that belong to your teams."
            actions={
              <Button>
                <HugeiconsIcon icon={Add01Icon} data-icon="inline-start" />
                Create API key
              </Button>
            }
          />
          <div className="grid gap-4 sm:grid-cols-3">
            {[
              { label: 'Active keys', value: '3' },
              { label: 'Requests (7d)', value: '12,480' },
              { label: 'Spend (7d)', value: '$49.78' },
            ].map((stat) => (
              <Card key={stat.label} size="sm">
                <CardHeader>
                  <CardDescription>{stat.label}</CardDescription>
                  <CardTitle className="text-2xl tabular-nums">{stat.value}</CardTitle>
                </CardHeader>
              </Card>
            ))}
          </div>
          <div className="flex items-center gap-3 rounded-lg border p-3">
            <HugeiconsIcon icon={Key01Icon} className="text-muted-foreground" />
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm font-medium">ci-pipeline</p>
              <p className="text-muted-foreground truncate font-mono text-xs">key_01JB7Q4M</p>
            </div>
            <Badge variant="success">Active</Badge>
          </div>
        </div>
      </ShowcaseBlock>
    </ShowcaseSection>
  )
}
