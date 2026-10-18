import { HugeiconsIcon } from '@hugeicons/react'
import { ArrowDown01Icon, Copy01Icon, MoreHorizontalIcon } from '@hugeicons/core-free-icons'
import { useState } from 'react'

import { AdminTableHeader } from '@/components/shared/admin-table-header'
import { Avatar, AvatarFallback, AvatarGroup, AvatarGroupCount } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from '@/components/ui/breadcrumb'
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
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import {
  Pagination,
  PaginationContent,
  PaginationEllipsis,
  PaginationItem,
  PaginationLink,
  PaginationNext,
  PaginationPrevious,
} from '@/components/ui/pagination'
import { Separator } from '@/components/ui/separator'
import { Table, TableBody, TableCaption, TableCell, TableRow } from '@/components/ui/table'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'

import { ShowcaseBlock, ShowcaseRow, ShowcaseSection } from './showcase'
import { ShowcaseChart } from './showcase-chart'
import { ShowcaseSectionNav } from './showcase-section-nav'
import { ShowcaseSidebar } from './showcase-sidebar'

const apiKeys = [
  { name: 'ci-pipeline', id: 'key_01JB7Q4M', owner: 'Platform', status: 'Active', spend: '$42.18' },
  { name: 'notebook', id: 'key_01JC2R9X', owner: 'Alice', status: 'Expiring', spend: '$7.60' },
  { name: 'legacy-bot', id: 'key_01H8ZK3T', owner: 'Data', status: 'Revoked', spend: '$0.00' },
] as const

const statusVariant = {
  Active: 'success',
  Expiring: 'warning',
  Revoked: 'destructive',
} as const

export function DataDisplaySection() {
  return (
    <ShowcaseSection
      id="data-display"
      title="Data display"
      description="Cards, tables, charts and identity. Cards sit one step above the content panel, separated by a hairline and a faint top highlight rather than a shadow."
    >
      <div className="grid gap-4 lg:grid-cols-3">
        <Card>
          <CardHeader>
            <CardTitle>Default card</CardTitle>
            <CardDescription>Header, content and footer, gap-4 inside.</CardDescription>
            <CardAction>
              <Button variant="ghost" size="icon-sm" aria-label="Card actions">
                <HugeiconsIcon icon={MoreHorizontalIcon} />
              </Button>
            </CardAction>
          </CardHeader>
          <CardContent>
            <p className="text-2xl font-semibold tabular-nums">1.28M</p>
            <p className="text-muted-foreground text-sm">Tokens this week</p>
          </CardContent>
          <CardFooter>
            <Button variant="outline" size="sm">
              View usage
            </Button>
          </CardFooter>
        </Card>
        <Card size="sm">
          <CardHeader>
            <CardTitle>Small card</CardTitle>
            <CardDescription>size=&quot;sm&quot; for dense stat grids.</CardDescription>
          </CardHeader>
          <CardContent>
            <p className="text-2xl font-semibold tabular-nums">$186.40</p>
            <p className="text-muted-foreground text-sm">of $250.00 weekly budget</p>
          </CardContent>
        </Card>
        <IdentityCard />
      </div>

      <ShowcaseBlock
        title="Table"
        description="Bordered list pages. AdminTableHeader draws the header row on --surface-muted with muted semibold labels; pass column labels, or AdminTableHead cells for headers that hold controls. Identifiers are monospace."
      >
        <div className="overflow-hidden rounded-lg border">
          <Table>
            <TableCaption className="sr-only">API keys</TableCaption>
            <AdminTableHeader
              columns={[
                'Name',
                'Key ID',
                'Owner',
                'Status',
                { label: 'Spend (7d)', align: 'right' },
                'Actions',
              ]}
            />
            <TableBody>
              {apiKeys.map((key) => (
                <TableRow key={key.id}>
                  <TableCell className="px-3 py-3 font-medium">{key.name}</TableCell>
                  <TableCell className="text-muted-foreground px-3 py-3 font-mono text-xs">
                    {key.id}
                  </TableCell>
                  <TableCell className="px-3 py-3">{key.owner}</TableCell>
                  <TableCell className="px-3 py-3">
                    <Badge variant={statusVariant[key.status]}>{key.status}</Badge>
                  </TableCell>
                  <TableCell className="px-3 py-3 text-right tabular-nums">{key.spend}</TableCell>
                  <TableCell className="px-3 py-3">
                    <Button variant="ghost" size="icon-xs" aria-label={`Copy ${key.id}`}>
                      <HugeiconsIcon icon={Copy01Icon} />
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
        <Pagination className="justify-end">
          <PaginationContent>
            <PaginationItem>
              <PaginationPrevious href="#data-display" />
            </PaginationItem>
            <PaginationItem>
              <PaginationLink href="#data-display" isActive>
                1
              </PaginationLink>
            </PaginationItem>
            <PaginationItem>
              <PaginationLink href="#data-display">2</PaginationLink>
            </PaginationItem>
            <PaginationItem>
              <PaginationEllipsis />
            </PaginationItem>
            <PaginationItem>
              <PaginationNext href="#data-display" />
            </PaginationItem>
          </PaginationContent>
        </Pagination>
      </ShowcaseBlock>

      <ShowcaseBlock
        title="Chart"
        description="Tokyo Night series in a fixed order: blue, green, teal, cyan, purple, then grey for Other. Data colours never mark actions, and lilac never marks data."
      >
        <ShowcaseChart />
      </ShowcaseBlock>
    </ShowcaseSection>
  )
}

function IdentityCard() {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Identity</CardTitle>
        <CardDescription>Generated avatars for people and teams.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="flex items-center gap-3">
          <GeneratedAvatar kind="user" name="Alice Example" size={40} />
          <div className="min-w-0">
            <p className="truncate text-sm font-medium">Alice Example</p>
            <p className="text-muted-foreground truncate text-xs">alice@platform.local</p>
          </div>
        </div>
        <Separator />
        <div className="flex items-center gap-3">
          <GeneratedAvatar kind="team" name="Platform" size={32} />
          <AvatarGroup>
            <Avatar size="sm">
              <AvatarFallback>AE</AvatarFallback>
            </Avatar>
            <Avatar size="sm">
              <AvatarFallback>BK</AvatarFallback>
            </Avatar>
            <AvatarGroupCount>+4</AvatarGroupCount>
          </AvatarGroup>
        </div>
      </CardContent>
    </Card>
  )
}

export function NavigationSection() {
  const [open, setOpen] = useState(false)

  return (
    <ShowcaseSection
      id="navigation"
      title="Navigation"
      description="Sidebar for the app shell, Tabs for peer views, Breadcrumb for depth, Collapsible for optional detail."
    >
      <div className="grid gap-4 lg:grid-cols-2">
        <ShowcaseBlock
          title="Sidebar"
          description="The darkest layer, framing the floating content panel. The active row uses --sidebar-accent with a lilac icon."
        >
          <ShowcaseSidebar />
        </ShowcaseBlock>

        <div className="flex min-w-0 flex-col gap-4">
          <ShowcaseBlock title="Breadcrumb" description="Detail pages under a list.">
            <Breadcrumb>
              <BreadcrumbList>
                <BreadcrumbItem>
                  <BreadcrumbLink href="#navigation">Identity</BreadcrumbLink>
                </BreadcrumbItem>
                <BreadcrumbSeparator />
                <BreadcrumbItem>
                  <BreadcrumbLink href="#navigation">Teams</BreadcrumbLink>
                </BreadcrumbItem>
                <BreadcrumbSeparator />
                <BreadcrumbItem>
                  <BreadcrumbPage>Platform</BreadcrumbPage>
                </BreadcrumbItem>
              </BreadcrumbList>
            </Breadcrumb>
          </ShowcaseBlock>

          <ShowcaseBlock
            title="Tabs"
            description="Default for segmented views, line for page-level tabs."
          >
            <ShowcaseRow label="default">
              <Tabs defaultValue="usage">
                <TabsList>
                  <TabsTrigger value="usage">Usage</TabsTrigger>
                  <TabsTrigger value="keys">Keys</TabsTrigger>
                  <TabsTrigger value="members" disabled>
                    Members
                  </TabsTrigger>
                </TabsList>
              </Tabs>
            </ShowcaseRow>
            <Tabs defaultValue="overview">
              <TabsList variant="line">
                <TabsTrigger value="overview">Overview</TabsTrigger>
                <TabsTrigger value="budget">Budget</TabsTrigger>
              </TabsList>
              <TabsContent value="overview" className="text-muted-foreground pt-3 text-sm">
                Line tabs sit on the page, not in a pill.
              </TabsContent>
              <TabsContent value="budget" className="text-muted-foreground pt-3 text-sm">
                Budget settings for this team.
              </TabsContent>
            </Tabs>
          </ShowcaseBlock>

          <ShowcaseBlock title="Collapsible" description="Advanced options that most people skip.">
            <Collapsible open={open} onOpenChange={setOpen} className="flex flex-col gap-2">
              <CollapsibleTrigger asChild>
                <Button variant="ghost" size="sm" className="w-fit">
                  Advanced options
                  <HugeiconsIcon
                    icon={ArrowDown01Icon}
                    data-icon="inline-end"
                    className={open ? 'rotate-180' : undefined}
                  />
                </Button>
              </CollapsibleTrigger>
              <CollapsibleContent className="text-muted-foreground rounded-lg border p-3 text-sm">
                Rate limit overrides and allowed IP ranges live here.
              </CollapsibleContent>
            </Collapsible>
          </ShowcaseBlock>
        </div>
      </div>

      <ShowcaseBlock
        title="Detail dialog sections"
        description="SectionNavSidebar lists a detail dialog's sections down its left edge from md up; SectionNavTabs repeats them as buttons in the dialog header below md."
      >
        <ShowcaseSectionNav />
      </ShowcaseBlock>
    </ShowcaseSection>
  )
}
