export type EntityTag = {
  key: string
  value: string
}

export function sanitizeEntityTags(tags: EntityTag[]) {
  const sanitizedTags: EntityTag[] = []
  for (const tag of tags) {
    const sanitizedTag = { key: tag.key.trim(), value: tag.value.trim() }
    if (sanitizedTag.key.length > 0 || sanitizedTag.value.length > 0) {
      sanitizedTags.push(sanitizedTag)
    }
  }
  return sanitizedTags
}
