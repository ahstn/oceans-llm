import { createFileRoute } from '@tanstack/react-router'

import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { getUsers } from '@/server/admin-data.functions'

export const Route = createFileRoute('/identity/users')({
  loader: () => getUsers(),
  component: UsersPage,
})

function UsersPage() {
  const { data } = Route.useLoaderData()

  return (
    <Card>
      <CardHeader>
        <CardTitle>Users</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="overflow-hidden rounded-md border border-neutral-800">
          <table className="w-full text-left text-sm">
            <thead className="bg-neutral-900/70 text-neutral-400">
              <tr>
                <th className="px-3 py-2 font-medium">Email</th>
                <th className="px-3 py-2 font-medium">Role</th>
                <th className="px-3 py-2 font-medium">Team</th>
                <th className="px-3 py-2 font-medium">Status</th>
              </tr>
            </thead>
            <tbody>
              {data.map((user) => (
                <tr key={user.id} className="border-t border-neutral-800">
                  <td className="px-3 py-2 text-neutral-100">{user.email}</td>
                  <td className="px-3 py-2 text-neutral-300">{user.role}</td>
                  <td className="px-3 py-2 text-neutral-300">{user.team}</td>
                  <td className="px-3 py-2">
                    <Badge variant={user.status === 'active' ? 'success' : 'warning'}>
                      {user.status}
                    </Badge>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </CardContent>
    </Card>
  )
}
