import { getModelOptions } from '@/server/admin-data.functions'
import type { ModelOptionView } from '@/types/api'

/**
 * Every gateway model, aliases included, for selectors that must offer the whole catalog.
 * One request: the options endpoint lists the catalog without the per-model pricing and client
 * config work of the paginated models list. Aborting `signal` cancels the load, such as when the
 * selector unmounts.
 */
export async function loadAllModels(signal?: AbortSignal): Promise<ModelOptionView[]> {
  const response = await getModelOptions({ signal })
  return response.data.items
}
