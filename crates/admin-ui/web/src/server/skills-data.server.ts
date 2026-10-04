import {
  createGatewayApiClient,
  fetchGatewayJson,
  resolveBrowserGatewayOrigin,
  unwrapGatewayResponse,
} from '@/server/gateway-client.server'
import type { SkillUploadResponse } from '@/types/skills-api'

export async function listSkills(input: { offset: number; namespace?: string; q?: string }) {
  return unwrapGatewayResponse(
    await createGatewayApiClient().GET('/api/v1/skills', {
      params: {
        query: { limit: 50, offset: input.offset, namespace: input.namespace, q: input.q },
      },
    }),
  )
}

export async function getSkillNamespace() {
  return unwrapGatewayResponse(await createGatewayApiClient().GET('/api/v1/skills/namespace'))
}

export async function getSkillLimits() {
  return unwrapGatewayResponse(await createGatewayApiClient().GET('/api/v1/skills/limits'))
}

export async function claimSkillNamespace(handle: string) {
  return unwrapGatewayResponse(
    await createGatewayApiClient().POST('/api/v1/skills/namespace', { body: { handle } }),
  )
}

export async function loadSkill(id: string, requestedVersion?: number) {
  const client = createGatewayApiClient()
  const detail = unwrapGatewayResponse(
    await client.GET('/api/v1/skills/{skill_id}', { params: { path: { skill_id: id } } }),
  )
  const version = unwrapGatewayResponse(
    await client.GET('/api/v1/skills/{skill_id}/versions/{version}', {
      params: { path: { skill_id: id, version: requestedVersion ?? detail.skill.default_version } },
    }),
  )
  // Project supported fields so the server-function serializer sees a finite JSON shape.
  const {
    name,
    description,
    license,
    compatibility,
    metadata,
    'allowed-tools': allowedTools,
  } = version.manifest
  return {
    detail,
    version: {
      ...version,
      manifest: {
        name,
        description,
        license,
        compatibility,
        metadata,
        'allowed-tools': allowedTools,
      },
    },
    limits: await getSkillLimits(),
    gatewayOrigin: resolveBrowserGatewayOrigin(),
  }
}

export async function readSkillFile(input: { id: string; version: number; path: string }) {
  return unwrapGatewayResponse(
    await createGatewayApiClient().GET('/api/v1/skills/{skill_id}/versions/{version}/files', {
      params: { path: { skill_id: input.id, version: input.version }, query: { path: input.path } },
    }),
  )
}

export async function setSkillDefault(input: { id: string; version: number }) {
  return unwrapGatewayResponse(
    await createGatewayApiClient().PUT('/api/v1/skills/{skill_id}/default-version', {
      params: { path: { skill_id: input.id } },
      body: { version: input.version },
    }),
  )
}

export async function uploadSkillArchive(data: FormData): Promise<SkillUploadResponse> {
  const archive = data.get('archive')
  const id = data.get('id')
  if (!(archive instanceof File) || archive.size === 0) {
    throw new Error('Choose a nonempty ZIP archive.')
  }
  const limits = await getSkillLimits()
  if (archive.size > limits.max_archive_bytes) {
    throw new Error(
      `The archive exceeds the ${limits.max_archive_bytes.toLocaleString()} byte limit.`,
    )
  }
  if (id !== null && (typeof id !== 'string' || !/^[\da-f-]{36}$/i.test(id))) {
    throw new Error('A valid skill ID is required.')
  }
  const path = id ? `/api/v1/skills/${id}/versions` : '/api/v1/skills'
  // Preserve the ZIP bytes; JSON serialization would corrupt the archive.
  return fetchGatewayJson<SkillUploadResponse>(path, {
    method: 'POST',
    headers: { 'content-type': 'application/zip' },
    body: archive,
  })
}
