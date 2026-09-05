import { useState } from 'react'
import { Add01Icon, ArrowLeft01Icon, ArrowRight01Icon } from '@hugeicons/core-free-icons'
import { AppIcon } from '@/components/icons/app-icon'
import {
  Frame,
  FrameDescription,
  FrameFooter,
  FrameHeader,
  FramePanel,
  FrameTitle,
} from '@/components/reui/frame'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { ToolsetCandidateProps } from './model'
import {
  DraftSummary,
  NoToolsets,
  SelectedSetHeader,
  ToolCatalog,
  ToolsetMark,
  ToolsetStatus,
  ToolsetToolbar,
} from './shared'

type BuilderStep = 'details' | 'tools' | 'review'

export function GuidedCandidate(props: ToolsetCandidateProps) {
  const [step, setStep] = useState<BuilderStep>('details')
  const currentStep = props.selected ? step : 'details'
  const canReview = props.selected !== null && props.catalogState === 'ready'

  return (
    <div className="flex min-w-0 flex-col gap-7">
      <header className="flex flex-col gap-2">
        <p className="text-muted-foreground text-xs font-medium tracking-widest uppercase">
          Guided builder
        </p>
        <h1 className="text-2xl font-semibold tracking-tight">Tool Sets</h1>
        <p className="text-muted-foreground max-w-2xl text-sm leading-relaxed">
          Choose a set, build a focused tool selection, and review it before saving.
        </p>
      </header>

      <Tabs
        value={currentStep}
        onValueChange={(value) => setStep(value as BuilderStep)}
        className="min-w-0 gap-7"
      >
        <TabsList aria-label="Tool set builder steps" variant="line" className="w-full">
          <TabsTrigger value="details" aria-label="Choose set" className="gap-2">
            <Badge variant={currentStep === 'details' ? 'default' : 'outline'}>1</Badge>
            <span className="sm:hidden">Set</span>
            <span className="hidden sm:inline">Choose set</span>
          </TabsTrigger>
          <TabsTrigger
            value="tools"
            aria-label="Choose tools"
            disabled={!props.selected}
            className="gap-2"
          >
            <Badge variant={currentStep === 'tools' ? 'default' : 'outline'}>2</Badge>
            <span className="sm:hidden">Tools</span>
            <span className="hidden sm:inline">Choose tools</span>
          </TabsTrigger>
          <TabsTrigger value="review" aria-label="Review" disabled={!canReview} className="gap-2">
            <Badge variant={currentStep === 'review' ? 'default' : 'outline'}>3</Badge>
            Review
          </TabsTrigger>
        </TabsList>

        <TabsContent value="details" className="min-w-0">
          <ChooseSetStep {...props} onContinue={() => setStep('tools')} />
        </TabsContent>
        <TabsContent value="tools" className="min-w-0">
          <ChooseToolsStep
            {...props}
            onBack={() => setStep('details')}
            onContinue={() => setStep('review')}
          />
        </TabsContent>
        <TabsContent value="review" className="min-w-0">
          <ReviewStep {...props} onBack={() => setStep('tools')} />
        </TabsContent>
      </Tabs>
    </div>
  )
}

function ChooseSetStep(props: ToolsetCandidateProps & { onContinue: () => void }) {
  return (
    <div className="grid min-w-0 items-start gap-7 xl:grid-cols-[minmax(0,1.15fr)_minmax(0,0.85fr)]">
      <section aria-labelledby="choose-toolset-heading" className="flex min-w-0 flex-col gap-5">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="flex flex-col gap-1">
            <h2 id="choose-toolset-heading" className="text-lg font-semibold tracking-tight">
              Start with a tool set
            </h2>
            <p className="text-muted-foreground text-sm">
              Update an existing set or create a new one.
            </p>
          </div>
          <Button type="button" variant="outline" onClick={props.onCreate}>
            <AppIcon icon={Add01Icon} aria-hidden data-icon="inline-start" />
            New tool set
          </Button>
        </div>
        <ToolsetToolbar {...props} />
        {props.sets.length > 0 ? (
          <ToggleGroup
            type="single"
            value={props.selected?.id ?? ''}
            onValueChange={(id) => {
              const set = props.sets.find((item) => item.id === id)
              if (set) props.onSelect(set)
            }}
            orientation="vertical"
            spacing={2}
            variant="outline"
            aria-label="Choose a tool set"
            className="w-full"
          >
            {props.sets.map((set) => (
              <ToggleGroupItem
                key={set.id}
                value={set.id}
                className="h-auto w-full justify-start gap-3 p-4 text-left whitespace-normal"
              >
                <ToolsetMark />
                <span className="flex min-w-0 flex-1 flex-col gap-1.5">
                  <span className="flex flex-wrap items-center justify-between gap-2">
                    <span className="min-w-0 wrap-anywhere">{set.display_name}</span>
                    <ToolsetStatus set={set} />
                  </span>
                  <span className="text-muted-foreground line-clamp-2 text-xs leading-relaxed font-normal">
                    {set.description || 'No description added.'}
                  </span>
                </span>
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        ) : (
          <NoToolsets onCreate={props.onCreate} />
        )}
      </section>

      <Frame spacing="lg" className="min-w-0">
        <FrameHeader>
          <FrameTitle>Your starting point</FrameTitle>
        </FrameHeader>
        <FramePanel className="flex min-w-0 flex-col gap-6">
          {props.selected ? (
            <SelectedSetHeader {...props} />
          ) : (
            <div className="flex flex-col items-start gap-3 py-5">
              <ToolsetMark />
              <h3 className="text-base font-semibold">Choose where your tools will live</h3>
              <p className="text-muted-foreground text-sm leading-relaxed">
                Select a tool set to review its details. You can then build a new selection from the
                available server tools.
              </p>
            </div>
          )}
          <Separator />
          <div className="flex flex-col gap-2">
            <h3 className="text-sm font-semibold">A small set with a clear purpose</h3>
            <p className="text-muted-foreground text-sm leading-relaxed">
              Group tools around a task, such as research or release planning. Choose only the tools
              that task needs, then manage who can use the set through access rules.
            </p>
          </div>
          <Button type="button" disabled={!props.selected} onClick={props.onContinue}>
            Continue to tools
            <AppIcon icon={ArrowRight01Icon} aria-hidden data-icon="inline-end" />
          </Button>
        </FramePanel>
        <FrameFooter>
          <FrameDescription>You will review the full replacement before saving.</FrameDescription>
        </FrameFooter>
      </Frame>
    </div>
  )
}

function ChooseToolsStep(
  props: ToolsetCandidateProps & { onBack: () => void; onContinue: () => void },
) {
  return (
    <div className="mx-auto flex w-full max-w-4xl min-w-0 flex-col gap-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <h2 className="text-lg font-semibold tracking-tight wrap-anywhere">
            Choose tools for {props.selected?.display_name}
          </h2>
          <p className="text-muted-foreground text-sm">
            Explore the available tools. Expand a row to inspect its inputs.
          </p>
        </div>
        <Badge variant="secondary">{props.draftIds.length} selected in this draft</Badge>
      </div>
      <ToolCatalog {...props} />
      <Separator />
      <div className="flex flex-wrap items-center justify-between gap-3">
        <Button type="button" variant="outline" onClick={props.onBack}>
          <AppIcon icon={ArrowLeft01Icon} aria-hidden data-icon="inline-start" />
          Back to details
        </Button>
        <Button
          type="button"
          disabled={!props.selected || props.catalogState !== 'ready'}
          onClick={props.onContinue}
        >
          Review selection
          <AppIcon icon={ArrowRight01Icon} aria-hidden data-icon="inline-end" />
        </Button>
      </div>
    </div>
  )
}

function ReviewStep(props: ToolsetCandidateProps & { onBack: () => void }) {
  return (
    <div className="mx-auto flex w-full max-w-3xl min-w-0 flex-col gap-5">
      <div className="flex flex-col gap-1">
        <h2 className="text-lg font-semibold tracking-tight">Review your replacement</h2>
        <p className="text-muted-foreground text-sm leading-relaxed">
          Check the target set and the full tool selection before you apply this change.
        </p>
      </div>
      <Frame spacing="lg">
        <FrameHeader>
          <FrameTitle>Final review</FrameTitle>
        </FrameHeader>
        <FramePanel className="flex min-w-0 flex-col gap-6">
          <SelectedSetHeader {...props} />
          <Separator />
          <DraftSummary {...props} />
        </FramePanel>
      </Frame>
      <Button type="button" variant="ghost" className="self-start" onClick={props.onBack}>
        <AppIcon icon={ArrowLeft01Icon} aria-hidden data-icon="inline-start" />
        Back to tools
      </Button>
    </div>
  )
}
