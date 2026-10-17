import { HugeiconsIcon } from '@hugeicons/react'
import {
  Add01Icon,
  Copy01Icon,
  Delete02Icon,
  FilterIcon,
  MoreHorizontalIcon,
  Search01Icon,
  Settings02Icon,
} from '@hugeicons/core-free-icons'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
  InputGroupText,
} from '@/components/ui/input-group'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Spinner } from '@/components/ui/spinner'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { Toggle } from '@/components/ui/toggle'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

import { previewState } from './preview-state'
import { ShowcaseBlock, ShowcaseRow, ShowcaseSection, StateMatrix } from './showcase'

const buttonVariants = ['default', 'secondary', 'outline', 'ghost', 'destructive', 'link'] as const
const buttonSizes = ['xs', 'sm', 'default', 'lg'] as const
const iconSizes = ['icon-xs', 'icon-sm', 'icon', 'icon-lg'] as const
const badgeVariants = [
  'default',
  'secondary',
  'success',
  'warning',
  'destructive',
  'outline',
  'ghost',
  'link',
] as const

export function ActionsSection() {
  return (
    <ShowcaseSection
      id="actions"
      title="Actions"
      description="Buttons, badges and toggles. One primary action per group; destructive actions stay tinted until confirmed."
    >
      <ShowcaseBlock
        title="Button"
        description="Variant × state. Hover, focus and active are pinned with data-preview, so this table needs no pointer."
      >
        <StateMatrix
          rows={buttonVariants}
          render={(variant, state) => (
            <Button variant={variant} {...previewState(state)}>
              {variant === 'destructive' ? 'Revoke' : 'Save changes'}
            </Button>
          )}
        />
        <ShowcaseRow label="Sizes">
          {buttonSizes.map((size) => (
            <Button key={size} size={size} variant="outline">
              {size}
            </Button>
          ))}
        </ShowcaseRow>
        <ShowcaseRow label="Icon buttons (always with an aria-label)">
          {iconSizes.map((size) => (
            <Button key={size} size={size} variant="ghost" aria-label={`More actions, ${size}`}>
              <HugeiconsIcon icon={MoreHorizontalIcon} />
            </Button>
          ))}
          <Button size="icon" variant="outline" aria-label="Copy key ID">
            <HugeiconsIcon icon={Copy01Icon} />
          </Button>
          <Button size="icon" variant="destructive" aria-label="Delete">
            <HugeiconsIcon icon={Delete02Icon} />
          </Button>
        </ShowcaseRow>
        <ShowcaseRow label="With icons and loading">
          <Button>
            <HugeiconsIcon icon={Add01Icon} data-icon="inline-start" />
            Create API key
          </Button>
          <Button variant="outline">
            <HugeiconsIcon icon={Settings02Icon} data-icon="inline-start" />
            Settings
          </Button>
          <Button disabled>
            <Spinner data-icon="inline-start" />
            Saving…
          </Button>
        </ShowcaseRow>
      </ShowcaseBlock>

      <div className="grid gap-4 lg:grid-cols-2">
        <ShowcaseBlock
          title="Badge"
          description="Status and metadata. Status colours describe state; never use them for selection or categories."
        >
          <ShowcaseRow label="Variants">
            {badgeVariants.map((variant) => (
              <Badge key={variant} variant={variant}>
                {variant}
              </Badge>
            ))}
          </ShowcaseRow>
          <ShowcaseRow label="In context">
            <Badge variant="success">Active</Badge>
            <Badge variant="warning">Expires in 3 days</Badge>
            <Badge variant="destructive">Revoked</Badge>
            <Badge variant="outline">platform_admin</Badge>
          </ShowcaseRow>
        </ShowcaseBlock>

        <ShowcaseBlock
          title="Toggle and ToggleGroup"
          description="Two to seven mutually exclusive options. Don't loop Buttons with manual active state."
        >
          <ShowcaseRow label="Toggle">
            <Toggle aria-label="Filter">
              <HugeiconsIcon icon={FilterIcon} />
            </Toggle>
            <Toggle variant="outline" defaultPressed>
              Errors only
            </Toggle>
            <Toggle variant="outline" disabled>
              Disabled
            </Toggle>
          </ShowcaseRow>
          <ShowcaseRow label="ToggleGroup, single">
            <ToggleGroup type="single" variant="outline" defaultValue="7d">
              <ToggleGroupItem value="24h">24h</ToggleGroupItem>
              <ToggleGroupItem value="7d">7d</ToggleGroupItem>
              <ToggleGroupItem value="30d">30d</ToggleGroupItem>
            </ToggleGroup>
          </ShowcaseRow>
          <ShowcaseRow label="ToggleGroup, small">
            <ToggleGroup type="single" size="sm" defaultValue="tokens">
              <ToggleGroupItem value="tokens">Tokens</ToggleGroupItem>
              <ToggleGroupItem value="spend">Spend</ToggleGroupItem>
            </ToggleGroup>
          </ShowcaseRow>
        </ShowcaseBlock>
      </div>
    </ShowcaseSection>
  )
}

export function InputsSection() {
  return (
    <ShowcaseSection
      id="inputs"
      title="Inputs"
      description="Forms compose Field inside FieldGroup. Invalid state is data-invalid on Field plus aria-invalid on the control."
    >
      <div className="grid gap-4 lg:grid-cols-2">
        <ShowcaseBlock
          title="Text fields"
          description="Input, InputGroup and Textarea, h-8 by default."
        >
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="ds-key-name">Key name</FieldLabel>
              <Input id="ds-key-name" placeholder="ci-pipeline" />
              <FieldDescription>Shown in request logs next to the owner.</FieldDescription>
            </Field>
            <Field data-invalid>
              <FieldLabel htmlFor="ds-email">Email</FieldLabel>
              <Input id="ds-email" defaultValue="alice@platform" aria-invalid />
              <FieldError>Enter a full email address.</FieldError>
            </Field>
            <Field data-disabled>
              <FieldLabel htmlFor="ds-team">Team</FieldLabel>
              <Input id="ds-team" defaultValue="Platform" disabled />
            </Field>
            <Field>
              <FieldLabel htmlFor="ds-search">Search</FieldLabel>
              <InputGroup>
                <InputGroupAddon>
                  <HugeiconsIcon icon={Search01Icon} />
                </InputGroupAddon>
                <InputGroupInput id="ds-search" placeholder="Search models" />
                <InputGroupAddon align="inline-end">
                  <InputGroupText>⌘K</InputGroupText>
                </InputGroupAddon>
              </InputGroup>
            </Field>
            <Field>
              <FieldLabel htmlFor="ds-budget">Weekly budget</FieldLabel>
              <InputGroup>
                <InputGroupAddon>
                  <InputGroupText>$</InputGroupText>
                </InputGroupAddon>
                <InputGroupInput id="ds-budget" inputMode="decimal" defaultValue="250.00" />
                <InputGroupAddon align="inline-end">
                  <InputGroupButton size="xs">Reset</InputGroupButton>
                </InputGroupAddon>
              </InputGroup>
            </Field>
            <Field>
              <FieldLabel htmlFor="ds-notes">Notes</FieldLabel>
              <Textarea id="ds-notes" placeholder="Why does this key exist?" />
            </Field>
          </FieldGroup>
        </ShowcaseBlock>

        <ChoiceBlock />
      </div>
    </ShowcaseSection>
  )
}

function ChoiceBlock() {
  return (
    <ShowcaseBlock title="Choices" description="Select, Checkbox and Switch with Label.">
      <FieldGroup>
        <Field>
          <FieldLabel htmlFor="ds-model">Default model</FieldLabel>
          <Select defaultValue="claude-sonnet">
            <SelectTrigger id="ds-model" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectLabel>Anthropic</SelectLabel>
                <SelectItem value="claude-sonnet">claude-sonnet-5-5</SelectItem>
                <SelectItem value="claude-haiku">claude-haiku-5-5</SelectItem>
              </SelectGroup>
              <SelectGroup>
                <SelectLabel>OpenAI</SelectLabel>
                <SelectItem value="gpt">gpt-5</SelectItem>
              </SelectGroup>
            </SelectContent>
          </Select>
        </Field>
        <Field data-invalid>
          <FieldLabel htmlFor="ds-owner">Owner</FieldLabel>
          <Select>
            <SelectTrigger id="ds-owner" className="w-full" aria-invalid>
              <SelectValue placeholder="Choose an owner" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value="alice">Alice</SelectItem>
                <SelectItem value="platform">Platform team</SelectItem>
              </SelectGroup>
            </SelectContent>
          </Select>
          <FieldError>Every key needs an owner.</FieldError>
        </Field>
        <FieldSet>
          <FieldLegend variant="label">Notify me about</FieldLegend>
          <Field orientation="horizontal">
            <Checkbox id="ds-budget-alerts" defaultChecked />
            <FieldLabel htmlFor="ds-budget-alerts" className="font-normal">
              Budget alerts
            </FieldLabel>
          </Field>
          <Field orientation="horizontal">
            <Checkbox id="ds-key-expiry" />
            <FieldLabel htmlFor="ds-key-expiry" className="font-normal">
              Key expiry
            </FieldLabel>
          </Field>
          <Field orientation="horizontal" data-disabled>
            <Checkbox id="ds-digest" disabled />
            <FieldLabel htmlFor="ds-digest" className="font-normal">
              Weekly digest (coming soon)
            </FieldLabel>
          </Field>
        </FieldSet>
        <Field orientation="horizontal">
          <FieldContent>
            <FieldLabel htmlFor="ds-enforce">Enforce budget</FieldLabel>
            <FieldDescription>Block requests once the weekly limit is reached.</FieldDescription>
          </FieldContent>
          <Switch id="ds-enforce" defaultChecked />
        </Field>
        <div className="flex items-center gap-2">
          <Switch id="ds-sso" disabled />
          <Label htmlFor="ds-sso">SSO only (disabled)</Label>
        </div>
      </FieldGroup>
    </ShowcaseBlock>
  )
}
