import { createGatewayApiClient, unwrapGatewayResponse } from '@/server/gateway-client.server'
import type {
  CreateMcpTagRuleInput,
  McpTagRulePreviewQuery,
  McpToolAnnotationsView,
} from '@/types/api'

export async function setMcpToolTags(input: { serverId: string; toolId: string; tags: string[] }) {
  return unwrapGatewayResponse(
    await createGatewayApiClient().PUT(
      '/api/v1/admin/mcp/servers/{server_id}/tools/{tool_id}/tags',
      {
        params: { path: { server_id: input.serverId, tool_id: input.toolId } },
        body: { tags: input.tags },
      },
    ),
  )
}

export async function setMcpToolAnnotationOverrides(input: {
  serverId: string
  toolId: string
  annotationOverrides: McpToolAnnotationsView
}) {
  return unwrapGatewayResponse(
    await createGatewayApiClient().PUT(
      '/api/v1/admin/mcp/servers/{server_id}/tools/{tool_id}/annotation-overrides',
      {
        params: { path: { server_id: input.serverId, tool_id: input.toolId } },
        body: { annotation_overrides: input.annotationOverrides },
      },
    ),
  )
}

export async function listMcpTagRules() {
  return unwrapGatewayResponse(await createGatewayApiClient().GET('/api/v1/admin/mcp/tag-rules'))
}

export async function createMcpTagRule(input: CreateMcpTagRuleInput) {
  return unwrapGatewayResponse(
    await createGatewayApiClient().POST('/api/v1/admin/mcp/tag-rules', { body: input }),
  )
}

export async function revokeMcpTagRule(ruleId: string) {
  return unwrapGatewayResponse(
    await createGatewayApiClient().DELETE('/api/v1/admin/mcp/tag-rules/{rule_id}', {
      params: { path: { rule_id: ruleId } },
    }),
  )
}

export async function previewMcpTagRule(query: McpTagRulePreviewQuery) {
  return unwrapGatewayResponse(
    await createGatewayApiClient().GET('/api/v1/admin/mcp/tag-rules/preview', {
      params: { query },
    }),
  )
}

/** The named versions of a tool, such as the two a review diff compares. */
export async function listMcpToolVersions(input: {
  serverId: string
  toolId: string
  schemaVersions: number[]
}) {
  return unwrapGatewayResponse(
    await createGatewayApiClient().GET(
      '/api/v1/admin/mcp/servers/{server_id}/tools/{tool_id}/versions',
      {
        params: {
          path: { server_id: input.serverId, tool_id: input.toolId },
          query: { schema_versions: input.schemaVersions.join(',') },
        },
      },
    ),
  )
}

export async function reviewMcpToolVersion(input: {
  serverId: string
  toolId: string
  schemaVersion: number
  decision: 'approve' | 'reject'
}) {
  const path = {
    server_id: input.serverId,
    tool_id: input.toolId,
    schema_version: input.schemaVersion,
  }
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    input.decision === 'approve'
      ? await client.POST(
          '/api/v1/admin/mcp/servers/{server_id}/tools/{tool_id}/versions/{schema_version}/approve',
          { params: { path } },
        )
      : await client.POST(
          '/api/v1/admin/mcp/servers/{server_id}/tools/{tool_id}/versions/{schema_version}/reject',
          { params: { path } },
        ),
  )
}
