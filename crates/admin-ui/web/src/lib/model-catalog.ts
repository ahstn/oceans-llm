import { getModels } from '@/server/admin-data.functions'
import type { ModelView } from '@/types/api'

/** The admin models endpoint clamps `page_size` to this. */
const MODEL_PAGE_SIZE = 100

type ModelPage = { data: { items: ModelView[]; total: number } }

function getModelPage(page: number) {
  return getModels({ data: { page, page_size: MODEL_PAGE_SIZE } }) as Promise<ModelPage>
}

/** Every gateway model, aliases included, for selectors that must offer the whole catalog. */
export async function loadAllModels(): Promise<ModelView[]> {
  const first = await getModelPage(1)
  const pageCount = Math.ceil(first.data.total / MODEL_PAGE_SIZE)
  const rest = await Promise.all(
    Array.from({ length: Math.max(0, pageCount - 1) }, (_, index) => getModelPage(index + 2)),
  )
  return [first, ...rest].flatMap((page) => page.data.items)
}
