import { useMemo, useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'

import { PageHeader } from '@/components/layout/page-header'
import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'

import { ManageKeysLink, ProfileApiKeysTable } from './-components/profile-api-keys'
import { BudgetCadenceBadge, BudgetMeter, NoBudget } from './-components/profile-budget'
import { RequestsBarChart, TokenVolumeChart } from './-components/profile-charts'
import {
  harnessRequestsChart,
  modelRequestsChart,
  profileInRange,
  tokenVolumeChart,
  type ProfileRange,
} from './-components/profile-data'
import { HeadlineTiles, profileHeadlines } from './-components/profile-headlines'
import { UsageHeatmap } from './-components/profile-heatmap'
import {
  loadProfilePage,
  profilePageModel,
  type ProfileLoaderData,
  type ProfilePageModel,
} from './-components/profile-page'
import { RangeToggle } from './-components/range-toggle'

export const Route = createFileRoute('/profile/')({
  loader: () => loadProfilePage(),
  component: ProfileOverviewPage,
})

/** Budget and headline tiles up top, then activity, trends and keys in stacked cards. */
export function ProfileOverviewPage() {
  const data = Route.useLoaderData() as ProfileLoaderData
  const { session } = Route.useRouteContext()
  if (!session) return null
  const model = profilePageModel(data, session)
  return <ProfileOverview model={model} name={session.user.name || session.user.email} />
}

export function ProfileOverview({ model, name }: { model: ProfilePageModel; name: string }) {
  const { profile, keys, links } = model
  const headlines = useMemo(() => profileHeadlines(profileInRange(profile, 30)), [profile])

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        leading={
          <GeneratedAvatar kind="user" name={name} className="rounded-xl" size={96} square />
        }
        section="Profile"
        title={`Welcome back, ${name.split(' ')[0]}`}
        description="Your budget, keys and gateway usage. Figures cover requests made with your personal API keys."
        actions={<ManageKeysLink links={links} />}
      />

      <div className="grid gap-4 lg:grid-cols-3">
        <Card className="lg:col-span-1">
          <CardHeader>
            <CardTitle>Budget</CardTitle>
            <CardDescription>Spend in the current period</CardDescription>
            {profile.budget ? (
              <CardAction>
                <BudgetCadenceBadge budget={profile.budget} />
              </CardAction>
            ) : null}
          </CardHeader>
          <CardContent>
            {profile.budget ? <BudgetMeter budget={profile.budget} size="lg" /> : <NoBudget />}
          </CardContent>
        </Card>
        <HeadlineTiles headlines={headlines} className="lg:col-span-2 lg:auto-rows-fr" />
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Activity</CardTitle>
          <CardDescription>
            Tokens per day over the last year. Hover a day for details.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <UsageHeatmap profile={profile} />
        </CardContent>
      </Card>

      <TrendsCard model={model} />

      <Card>
        <CardHeader>
          <CardTitle>API keys</CardTitle>
          <CardDescription>Personal keys you use to call the gateway.</CardDescription>
          <CardAction>
            <ManageKeysLink links={links} />
          </CardAction>
        </CardHeader>
        <CardContent>
          <ProfileApiKeysTable keys={keys} links={links} />
        </CardContent>
      </Card>
    </div>
  )
}

function TrendsCard({ model }: { model: ProfilePageModel }) {
  const [range, setRange] = useState<ProfileRange>(90)
  const weekly = range > 90
  const charts = useMemo(
    () => ({
      tokens: tokenVolumeChart(model.profile, range),
      models: modelRequestsChart(profileInRange(model.profile, range), range),
      harnesses: harnessRequestsChart(profileInRange(model.profile, range), range),
    }),
    [model.profile, range],
  )

  return (
    <Card>
      <Tabs defaultValue="tokens" className="gap-4">
        <CardHeader>
          <CardTitle>Trends</CardTitle>
          <CardDescription>{weekly ? 'Weekly totals' : 'Daily totals'}</CardDescription>
          <CardAction className="flex flex-wrap items-center gap-2">
            <TabsList>
              <TabsTrigger value="tokens">Token volume</TabsTrigger>
              <TabsTrigger value="models">Models</TabsTrigger>
              <TabsTrigger value="harnesses">Clients</TabsTrigger>
            </TabsList>
            <RangeToggle value={range} onChange={setRange} />
          </CardAction>
        </CardHeader>
        <CardContent>
          <TabsContent value="tokens">
            <TokenVolumeChart chart={charts.tokens} weekly={weekly} heightClass="h-72" />
          </TabsContent>
          <TabsContent value="models">
            <RequestsBarChart chart={charts.models} weekly={weekly} heightClass="h-72" />
          </TabsContent>
          <TabsContent value="harnesses">
            <RequestsBarChart chart={charts.harnesses} weekly={weekly} heightClass="h-72" />
          </TabsContent>
        </CardContent>
      </Tabs>
    </Card>
  )
}
