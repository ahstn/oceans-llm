import { getModels } from '@/server/admin-data.functions'
import type { ModelView } from '@/types/api'

/** The admin models endpoint clamps `page_size` to this. */
const MODEL_PAGE_SIZE = 100

type ModelPage = { data: { items: ModelView[]; total: number } }

function getModelPage(page: number, signal?: AbortSignal) {
  return getModels({ data: { page, page_size: MODEL_PAGE_SIZE }, signal }) as Promise<ModelPage>
}

/** At most this many page requests run at once, so a large catalog does not burst the API. */
const MAX_CONCURRENT_PAGES = 4

/**
 * Every gateway model, aliases included, for selectors that must offer the whole catalog.
 * Aborting `signal` cancels the load, such as when the selector unmounts.
 */
export async function loadAllModels(signal?: AbortSignal): Promise<ModelView[]> {
  const pending = new AbortController()
  if (signal?.aborted) pending.abort()
  signal?.addEventListener('abort', () => pending.abort(), { once: true })
  const first = await getModelPage(1, pending.signal)
  const pageCount = Math.ceil(first.data.total / MODEL_PAGE_SIZE)
  const rest: ModelPage[] = []
  let nextPage = 2
  // Each worker fetches one page at a time until none are left. After any failure the load is
  // already lost, so the workers stop claiming pages and the pages still in flight are cancelled.
  async function drain(): Promise<void> {
    if (pending.signal.aborted || nextPage > pageCount) return
    const page = nextPage++
    try {
      rest[page - 2] = await getModelPage(page, pending.signal)
    } catch (error) {
      pending.abort()
      throw error
    }
    return drain()
  }
  const workers = Math.min(MAX_CONCURRENT_PAGES, Math.max(0, pageCount - 1))
  await Promise.all(Array.from({ length: workers }, drain))
  return [first, ...rest].flatMap((page) => page.data.items)
}
