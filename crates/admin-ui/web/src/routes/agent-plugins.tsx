import { PuzzleIcon } from '@hugeicons/core-free-icons'
import { createFileRoute } from '@tanstack/react-router'

import { AppIcon } from '@/components/icons/app-icon'
import { PageHeader } from '@/components/layout/page-header'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from '@/components/ui/empty'

export const Route = createFileRoute('/agent-plugins')({
  component: AgentPluginsPage,
})

function AgentPluginsPage() {
  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Agent Extensions"
        title="Agent Plugins"
        description="Bundle MCP servers, skills and more into installable plugins for your agents."
      />
      <Card className="min-w-0">
        <CardHeader>
          <CardTitle>Plugin catalog</CardTitle>
          <CardDescription>Browse and install shared agent plugins.</CardDescription>
        </CardHeader>
        <CardContent>
          <Empty>
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <AppIcon icon={PuzzleIcon} />
              </EmptyMedia>
              <EmptyTitle>No plugins yet</EmptyTitle>
              <EmptyDescription>Agent plugins are coming soon.</EmptyDescription>
            </EmptyHeader>
          </Empty>
        </CardContent>
      </Card>
    </div>
  )
}
