export interface Paginated<T> {
  items: T[]
  page: number
  pageSize: number
  total: number
}

export interface ApiKeyView {
  id: string
  name: string
  prefix: string
  createdAt: string
  status: 'active' | 'revoked'
}

export interface ModelView {
  id: string
  provider: string
  upstreamModel: string
  tags: string[]
  status: 'healthy' | 'degraded'
}
