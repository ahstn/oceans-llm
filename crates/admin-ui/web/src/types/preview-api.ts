export interface Paginated<T> {
  items: T[]
  page: number
  pageSize: number
  total: number
}

export interface ModelView {
  id: string
  provider: string
  upstreamModel: string
  tags: string[]
  status: 'healthy' | 'degraded'
}
