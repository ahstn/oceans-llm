import { createFileRoute, redirect } from '@tanstack/react-router'

import { defaultSignedInPath } from '@/routes/-auth-routing'

export const Route = createFileRoute('/')({
  beforeLoad: ({ context }) => {
    if (context.session) {
      throw redirect({ to: defaultSignedInPath(context.session) })
    }
  },
})
