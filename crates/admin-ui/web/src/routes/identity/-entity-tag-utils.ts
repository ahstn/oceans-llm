export type EntityTag = {
  key: string
  value: string
}

export function sanitizeEntityTags(tags: EntityTag[]) {
  return tags
    .map((tag) => ({ key: tag.key.trim(), value: tag.value.trim() }))
    .filter((tag) => tag.key.length > 0 || tag.value.length > 0)
}
