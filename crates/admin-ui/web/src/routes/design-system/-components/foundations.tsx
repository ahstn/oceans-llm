import { HugeiconsIcon } from '@hugeicons/react'
import {
  Key01Icon,
  McpServerIcon,
  RefreshIcon,
  SaveMoneyDollarIcon,
  SecurityCheckIcon,
  UserCircleIcon,
} from '@hugeicons/core-free-icons'
import { Link } from '@tanstack/react-router'
import { useState, type ReactNode } from 'react'

import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Progress } from '@/components/ui/progress'
import { cn } from '@/lib/utils'

import { ColorPair, ShowcaseBlock, ShowcaseSection, Swatch } from './showcase'

const colorRoles = [
  { name: 'Frame', background: 'sidebar', foreground: 'sidebar-foreground' },
  { name: 'Content panel', background: 'background', foreground: 'foreground' },
  { name: 'Card', background: 'card', foreground: 'card-foreground' },
  { name: 'Floating layer', background: 'popover', foreground: 'popover-foreground' },
  { name: 'Primary action', background: 'primary', foreground: 'primary-foreground' },
  { name: 'Secondary', background: 'secondary', foreground: 'secondary-foreground' },
  { name: 'Hover', background: 'accent', foreground: 'accent-foreground' },
  { name: 'Soft text on card', background: 'card', foreground: 'muted-foreground' },
  { name: 'Link text on card', background: 'card', foreground: 'primary' },
  { name: 'Focus ring on card', background: 'card', foreground: 'ring' },
  { name: 'Destructive on card', background: 'card', foreground: 'destructive' },
  { name: 'Success on card', background: 'card', foreground: 'success' },
] as const

const paletteGroups = [
  {
    label: 'Actions',
    swatches: [
      { name: 'Lilac', token: 'primary', note: 'Buttons, links, checked controls' },
      { name: 'Focus ring', token: 'ring', note: 'Keyboard focus' },
    ],
  },
  {
    label: 'Brand illustration',
    swatches: [
      { name: 'Logo ocean blue', token: 'sidebar-primary', note: 'Brand tile' },
      { name: 'Logo mist', token: 'mist', note: 'Logo and illustration' },
      { name: 'Wave foam', token: 'wave-foam', note: 'Hero crescent only' },
      { name: 'Wave deep', token: 'wave-deep', note: 'Hero shadow only' },
    ],
  },
  {
    label: 'Surfaces, darkest first',
    swatches: [
      { name: 'Frame', token: 'sidebar', note: 'Sidebar' },
      { name: 'Content panel', token: 'background' },
      { name: 'Card', token: 'card' },
      { name: 'Floating layer', token: 'popover' },
      { name: 'Table header', token: 'surface-muted' },
      { name: 'Hover wash', token: 'accent' },
    ],
  },
  {
    label: 'Text and lines',
    swatches: [
      { name: 'Ink', token: 'foreground' },
      { name: 'Soft text', token: 'muted-foreground' },
      { name: 'Nav text', token: 'sidebar-foreground' },
      { name: 'Hairline', token: 'border' },
      { name: 'Input line', token: 'input' },
    ],
  },
  {
    label: 'Status',
    swatches: [
      { name: 'Success', token: 'success', note: 'Tokyo Night green1' },
      { name: 'Warning', token: 'warning' },
      { name: 'Destructive', token: 'destructive', note: 'Rose-400, never an action' },
    ],
  },
  {
    label: 'Chart series (Tokyo Night)',
    swatches: [
      { name: 'Blue', token: 'chart-1' },
      { name: 'Green', token: 'chart-2' },
      { name: 'Teal', token: 'chart-3' },
      { name: 'Cyan', token: 'chart-4' },
      { name: 'Purple', token: 'chart-5' },
      { name: 'Other', token: 'chart-other', note: 'Grouped series' },
      { name: 'Hero', token: 'chart-hero', note: 'Single-series charts, meters' },
    ],
  },
  {
    label: 'Activity heatmap, empty to busiest',
    swatches: [
      { name: 'Empty', token: 'heatmap-0' },
      { name: 'Level 1', token: 'heatmap-1' },
      { name: 'Level 2', token: 'heatmap-2' },
      { name: 'Level 3', token: 'heatmap-3' },
      { name: 'Level 4', token: 'heatmap-4' },
    ],
  },
] as const

const elevationSteps = [
  { label: 'Frame', use: 'Sidebar, behind the panel', className: 'bg-sidebar' },
  {
    label: 'Panel',
    use: 'Content panel · shadow-panel',
    className: 'bg-background ring-border shadow-panel ring-1',
  },
  {
    label: 'Card',
    use: 'Cards · hairline, inset-shadow-highlight',
    className: 'bg-card ring-foreground/10 inset-shadow-highlight ring-1',
  },
  {
    label: 'Popover',
    use: 'Menus, selects · shadow-popover',
    className: 'bg-popover ring-foreground/10 shadow-popover ring-1',
  },
  {
    label: 'Dialog',
    use: 'Dialogs, sheets · shadow-dialog',
    className: 'bg-popover ring-foreground/10 shadow-dialog ring-1',
  },
] as const

const motionTokens = [
  {
    token: '--duration-fast',
    value: '120ms',
    easing: 'ease-standard',
    use: 'Hover, press, nav rows',
  },
  {
    token: '--duration-base',
    value: '200ms',
    easing: 'ease-emphasized',
    use: 'Page entrance, dialogs, overlays',
  },
  {
    token: '--duration-slow',
    value: '320ms',
    easing: 'ease-emphasized',
    use: 'Sheets, progress fills',
  },
  { token: '--duration-chart', value: '600ms', easing: 'ease-out', use: 'Chart series draw-in' },
] as const

const typeScale = [
  {
    token: 'type-display',
    spec: '36–48px / 1.1 / 500',
    use: 'Auth and error surfaces only',
    sample: <p className="type-display truncate">Secure the gateway.</p>,
  },
  {
    token: 'text-2xl font-semibold',
    spec: '24px / 1.33 / 600',
    use: 'Page titles (PageHeader)',
    sample: <p className="truncate text-2xl font-semibold">Welcome back, Alice</p>,
  },
  {
    token: 'type-headline',
    spec: '16px / 1.375 / 500',
    use: 'Card titles, section heads',
    sample: <p className="type-headline truncate">Budget</p>,
  },
  {
    token: 'type-title',
    spec: '14px / 1.25 / 500',
    use: 'Table headers, dialog titles',
    sample: <p className="type-title truncate">Owner user</p>,
  },
  {
    token: 'type-body',
    spec: '15–16px / 1.5 / 400',
    use: 'Reading text, form descriptions',
    sample: <p className="type-body truncate">Keys inherit the owner&apos;s budget and grants.</p>,
  },
  {
    token: 'type-label',
    spec: '14px / 1.25 / 500',
    use: 'Buttons, field labels, nav rows',
    sample: <p className="type-label truncate">Create API key</p>,
  },
  {
    token: 'type-caption',
    spec: '12px / 1.33 / 500',
    use: 'Badges, metadata, chart axes',
    sample: <p className="type-caption text-muted-foreground truncate">Resets Mon, 00:00 UTC</p>,
  },
  {
    token: 'font-mono text-sm',
    spec: '14px mono',
    use: 'Identifiers, keys, request IDs',
    sample: <p className="truncate font-mono text-sm">req_01JB7Q4M8Z2V</p>,
  },
] as const

const radiusSteps = [
  { label: 'sm', className: 'rounded-sm', value: '4.3px', use: 'Small accents' },
  { label: 'md', className: 'rounded-md', value: '5.8px', use: 'Menu items, tab triggers' },
  { label: 'lg', className: 'rounded-lg', value: '7.2px', use: 'Buttons, inputs, selects' },
  { label: 'xl', className: 'rounded-xl', value: '10.1px', use: 'Cards, dialogs, hero panel' },
  { label: '4xl', className: 'rounded-4xl', value: 'pill', use: 'Badges' },
] as const

const spacingSteps = [
  { label: 'xs', className: 'w-1', value: '4px', use: 'Icon to label' },
  { label: 'sm', className: 'w-2', value: '8px', use: 'Inside controls, button rows' },
  { label: 'md', className: 'w-4', value: '16px', use: 'Card padding, card grid gap' },
  { label: 'lg', className: 'w-6', value: '24px', use: 'Between page sections' },
  { label: 'xl', className: 'w-8', value: '32px', use: 'Auth layout groups' },
] as const

const featureIcons = [
  { name: 'user', icon: UserCircleIcon },
  { name: 'key', icon: Key01Icon },
  { name: 'mcp-server', icon: McpServerIcon },
  { name: 'spend', icon: SaveMoneyDollarIcon },
  { name: 'security', icon: SecurityCheckIcon },
  { name: 'refresh', icon: RefreshIcon },
] as const

export function FoundationsSection() {
  return (
    <ShowcaseSection
      id="foundations"
      title="Foundations"
      description="Color, type, radius, spacing, elevation and motion. Every value lives in src/styles/globals.css. Surfaces come from the account-ready art; actions and data colours come from Tokyo Night."
    >
      <ShowcaseBlock
        title="Color roles"
        description="Each surface with its foreground and the measured contrast. Pairs under 4.5:1 are not for body text."
      >
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          {colorRoles.map((role) => (
            <ColorPair key={role.name} {...role} />
          ))}
        </div>
      </ShowcaseBlock>

      <ShowcaseBlock
        title="Palette"
        description="Grouped the way DESIGN.md groups them. Lilac marks actions, Tokyo Night colours mark data and wave colours belong to illustration. None of them stands in for another."
      >
        {paletteGroups.map((group) => (
          <div key={group.label} className="flex flex-col gap-2">
            <p className="type-caption text-muted-foreground">{group.label}</p>
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-6">
              {group.swatches.map((swatch) => (
                <Swatch key={swatch.token} {...swatch} />
              ))}
            </div>
          </div>
        ))}
      </ShowcaseBlock>

      <TypographyBlock />

      <div className="grid gap-4 lg:grid-cols-2">
        <ShowcaseBlock title="Radius" description="Derived from --radius: 0.45rem.">
          <div className="flex flex-col gap-3">
            {radiusSteps.map((step) => (
              <div key={step.label} className="flex items-center gap-4">
                <div className={`bg-muted size-12 shrink-0 border ${step.className}`} />
                <p className="text-sm">
                  <span className="font-mono">{step.label}</span> · {step.value} ·{' '}
                  <span className="text-muted-foreground">{step.use}</span>
                </p>
              </div>
            ))}
          </div>
        </ShowcaseBlock>

        <ShowcaseBlock title="Spacing" description="Tailwind's 4px grid, used through gap-*.">
          <div className="flex flex-col gap-3">
            {spacingSteps.map((step) => (
              <div key={step.label} className="flex items-center gap-4">
                <div className="flex w-12 shrink-0">
                  <div className={`bg-primary h-4 rounded-sm ${step.className}`} />
                </div>
                <p className="text-sm">
                  <span className="font-mono">{step.label}</span> · {step.value} ·{' '}
                  <span className="text-muted-foreground">{step.use}</span>
                </p>
              </div>
            ))}
          </div>
        </ShowcaseBlock>
      </div>

      <ElevationBlock />
      <MotionBlock />
    </ShowcaseSection>
  )
}

// Token reference table shared by the type scale and motion tokens; rows come from the caller.
function SpecTable({ columns, children }: { columns: string[]; children: ReactNode }) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full min-w-xl text-left text-sm">
        <thead className="text-muted-foreground">
          <tr className="border-b">
            {columns.map((column) => (
              <th key={column} className="py-2 pr-4 font-normal last:pr-0">
                {column}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>{children}</tbody>
      </table>
    </div>
  )
}

function TypographyBlock() {
  return (
    <ShowcaseBlock
      title="Typography"
      description="One family, Geist Variable. Monospace only for machine-like strings."
    >
      <SpecTable columns={['Token', 'Spec', 'Use', 'Specimen']}>
        {typeScale.map((row) => (
          <tr key={row.token} className="border-b last:border-b-0">
            <td className="py-3 pr-4 font-mono text-xs whitespace-nowrap">{row.token}</td>
            <td className="text-muted-foreground py-3 pr-4 text-xs whitespace-nowrap">
              {row.spec}
            </td>
            <td className="text-muted-foreground py-3 pr-4 text-xs">{row.use}</td>
            <td className="max-w-xs py-3">{row.sample}</td>
          </tr>
        ))}
      </SpecTable>
      <p className="type-body text-muted-foreground max-w-2xl">
        Requests made with a revoked key fail with <code className="font-mono text-sm">401</code>{' '}
        and still appear in{' '}
        <Link to="/observability/request-logs" className="text-foreground underline">
          Request Logs
        </Link>
        , so operators can trace who tried to use it and when.
      </p>
      <div className="flex flex-wrap items-center gap-4">
        {featureIcons.map((item, index) => (
          <HugeiconsIcon
            key={item.name}
            icon={item.icon}
            className={index % 2 === 0 ? 'text-foreground' : 'text-muted-foreground'}
          />
        ))}
        <span className="text-muted-foreground text-xs">Hugeicons, 16–18px</span>
      </div>
    </ShowcaseBlock>
  )
}

function ElevationBlock() {
  return (
    <ShowcaseBlock
      title="Elevation"
      description="Five layers, darkest at the back. Cards rest on the panel with a hairline and a faint top highlight; only floating UI casts a shadow."
    >
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-5">
        {elevationSteps.map((step) => (
          <div
            key={step.label}
            className={cn('flex h-28 flex-col justify-end rounded-xl p-3', step.className)}
          >
            <p className="text-sm font-medium">{step.label}</p>
            <p className="text-muted-foreground text-xs">{step.use}</p>
          </div>
        ))}
      </div>
    </ShowcaseBlock>
  )
}

function MotionBlock() {
  const [run, setRun] = useState(0)

  return (
    <ShowcaseBlock
      title="Motion"
      description="One entrance per page, colour fades on hover and fills that glide to their value. All of it stops when the viewer prefers reduced motion."
    >
      <SpecTable columns={['Token', 'Duration', 'Easing', 'Use']}>
        {motionTokens.map((row) => (
          <tr key={row.token} className="border-b last:border-b-0">
            <td className="py-3 pr-4 font-mono text-xs whitespace-nowrap">{row.token}</td>
            <td className="py-3 pr-4 text-xs tabular-nums">{row.value}</td>
            <td className="text-muted-foreground py-3 pr-4 font-mono text-xs">{row.easing}</td>
            <td className="text-muted-foreground py-3 text-xs">{row.use}</td>
          </tr>
        ))}
      </SpecTable>
      <div className="flex flex-wrap items-center gap-4">
        <Button variant="outline" size="sm" onClick={() => setRun((count) => count + 1)}>
          <HugeiconsIcon icon={RefreshIcon} data-icon="inline-start" />
          Replay
        </Button>
        <Card key={run} size="sm" className="animate-page-in">
          <CardContent className="text-sm">Page entrance</CardContent>
        </Card>
        <Progress value={run % 2 === 0 ? 35 : 80} className="w-40" aria-label="Fill demo" />
      </div>
    </ShowcaseBlock>
  )
}
