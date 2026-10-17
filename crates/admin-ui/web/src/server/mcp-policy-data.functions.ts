import { createServerFn } from '@tanstack/react-start'

import {
  createMcpTagRule,
  listMcpTagRules,
  previewMcpTagRule,
  revokeMcpTagRule,
  setMcpToolAnnotationOverrides,
  setMcpToolTags,
} from '@/server/mcp-policy-data.server'
import type {
  CreateMcpTagRuleInput,
  McpTagRulePreviewQuery,
  McpToolAnnotationsView,
} from '@/types/api'

export const saveMcpToolTags = createServerFn({ method: 'POST' })
  .validator((data: { serverId: string; toolId: string; tags: string[] }) => data)
  .handler(({ data }) => setMcpToolTags(data))

export const saveMcpToolAnnotationOverrides = createServerFn({ method: 'POST' })
  .validator(
    (data: { serverId: string; toolId: string; annotationOverrides: McpToolAnnotationsView }) =>
      data,
  )
  .handler(({ data }) => setMcpToolAnnotationOverrides(data))

export const getMcpTagRules = createServerFn({ method: 'GET' }).handler(() => listMcpTagRules())

export const saveMcpTagRule = createServerFn({ method: 'POST' })
  .validator((data: CreateMcpTagRuleInput) => data)
  .handler(({ data }) => createMcpTagRule(data))

export const removeMcpTagRule = createServerFn({ method: 'POST' })
  .validator((data: { ruleId: string }) => data)
  .handler(({ data }) => revokeMcpTagRule(data.ruleId))

export const getMcpTagRulePreview = createServerFn({ method: 'GET' })
  .validator((data: McpTagRulePreviewQuery) => data)
  .handler(({ data }) => previewMcpTagRule(data))
