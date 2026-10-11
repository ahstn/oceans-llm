import { createCsrfMiddleware, createMiddleware, createStart } from '@tanstack/react-start'

import { getSkillLimits } from '@/server/skills-data.server'
import { checkMultipartBodySize, MULTIPART_OVERHEAD_BYTES } from '@/server/request-body.server'

// A custom start entry replaces TanStack's default middleware, so retain its CSRF policy.
const csrfMiddleware = createCsrfMiddleware({
  filter: ({ handlerType }) => handlerType === 'serverFn',
})
const multipartBodyLimit = createMiddleware().server(async ({ request, handlerType, next }) => {
  const contentType = request.headers.get('content-type')?.split(';')[0]?.trim().toLowerCase()
  if (handlerType !== 'serverFn' || contentType !== 'multipart/form-data') return next()

  // Authenticate and use the gateway's configured limit before parsing any upload fields.
  const limits = await getSkillLimits()
  const rejection = await checkMultipartBodySize(
    request,
    limits.max_archive_bytes + MULTIPART_OVERHEAD_BYTES,
  )
  return rejection ?? next()
})

export const startInstance = createStart(() => ({
  requestMiddleware: [csrfMiddleware, multipartBodyLimit],
}))
