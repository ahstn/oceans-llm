import { createFileRoute, redirect } from '@tanstack/react-router'

import { DEFAULT_SIGNED_IN_PATH } from '@/routes/-auth-routing'

export const Route = createFileRoute('/')({
  beforeLoad: () => {
    throw redirect({ to: DEFAULT_SIGNED_IN_PATH })
  },
})
