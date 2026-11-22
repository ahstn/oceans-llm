import { getModels } from '@/server/admin-data.functions'
import type { ModelView } from '@/types/api'

/** The admin models endpoint clamps `page_size` to this. */
const MODEL_PAGE_SIZE = 100

type ModelPage = { data: { items: ModelView[]; total: number } }

function getModelPage(page: number) {
  return getModels({ data: { page, page_size: MODEL_PAGE_SIZE } }) as Promise<ModelPage>
}

/** At most this many page requests run at once, so a large catalog does not burst the API. */
const MAX_CONCURRENT_PAGES = 4

/** Every gateway model, aliases included, for selectors that must offer the whole catalog. */
export async function loadAllModels(): Promise<ModelView[]> {
  const first = await getModelPage(1)
  const pageCount = Math.ceil(first.data.total / MODEL_PAGE_SIZE)
  const rest: ModelPage[] = []
  let nextPage = 2
  let failed = false
  // Each worker fetches one page at a time until none are left. After any failure the load is
  // already lost, so the workers stop claiming pages instead of fetching ones that are discarded.
  async function drain(): Promise<void> {
    if (failed || nextPage > pageCount) return
    const page = nextPage++
    try {
      rest[page - 2] = await getModelPage(page)
    } catch (error) {
      failed = true
      throw error
    }
    return drain()
  }
  const workers = Math.min(MAX_CONCURRENT_PAGES, Math.max(0, pageCount - 1))
  await Promise.all(Array.from({ length: workers }, drain))
  return [first, ...rest].flatMap((page) => page.data.items)
}
