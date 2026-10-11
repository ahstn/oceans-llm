import { createServerFn } from '@tanstack/react-start'

import {
  claimSkillNamespace,
  getSkillNamespace,
  getSkillLimits,
  listSkills,
  loadSkill,
  readSkillFile,
  setSkillDefault,
  uploadSkillArchive,
} from '@/server/skills-data.server'

export const getSkills = createServerFn({ method: 'GET' })
  .validator((data: { offset: number; namespace?: string; q?: string }) => data)
  .handler(async ({ data }) => {
    const [items, namespace, limits] = await Promise.all([
      listSkills(data),
      getSkillNamespace(),
      getSkillLimits(),
    ])
    return { items, namespace, limits }
  })

export const getSkill = createServerFn({ method: 'GET' })
  .validator((data: { id: string; version?: number }) => data)
  .handler(({ data }) => loadSkill(data.id, data.version))

export const getSkillFile = createServerFn({ method: 'GET' })
  .validator((data: { id: string; version: number; path: string }) => data)
  .handler(({ data }) => readSkillFile(data))

export const saveSkillNamespace = createServerFn({ method: 'POST' })
  .validator((data: { handle: string }) => data)
  .handler(({ data }) => claimSkillNamespace(data.handle))

export const saveSkillDefault = createServerFn({ method: 'POST' })
  .validator((data: { id: string; version: number }) => data)
  .handler(({ data }) => setSkillDefault(data))

export const uploadSkill = createServerFn({ method: 'POST' })
  .validator((data: FormData) => data)
  .handler(({ data }) => uploadSkillArchive(data))
