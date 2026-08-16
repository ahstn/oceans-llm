import { createFileRoute } from '@tanstack/react-router'

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'

export const Route = createFileRoute('/no-access')({
  component: NoAccessPage,
})

export function NoAccessPage() {
  return (
    <Card>
      <CardHeader>
        <CardTitle>No console pages available</CardTitle>
        <CardDescription>
          Your account is active, but your permission group has no console pages. Contact a platform
          admin if you need access.
        </CardDescription>
      </CardHeader>
      <CardContent className="text-muted-foreground text-sm">
        You can still change your password or sign out from the account menu.
      </CardContent>
    </Card>
  )
}
