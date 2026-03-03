import { createFileRoute } from '@tanstack/react-router'

import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { getTeams } from '@/server/admin-data.functions'

export const Route = createFileRoute('/identity/teams')({
  loader: () => getTeams(),
  component: TeamsPage,
})

function TeamsPage() {
  const { data } = Route.useLoaderData()

  return (
    <Card>
      <CardHeader>
        <CardTitle>Teams</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        {data.map((team) => (
          <div
            key={team.id}
            className="flex items-center justify-between rounded-md border border-neutral-800 bg-neutral-950/40 px-3 py-2"
          >
            <div>
              <p className="text-sm text-neutral-100">{team.name}</p>
              <p className="text-xs text-neutral-400">{team.users} users</p>
            </div>
            <Badge variant={team.status === 'active' ? 'success' : 'warning'}>{team.status}</Badge>
          </div>
        ))}
      </CardContent>
    </Card>
  )
}
