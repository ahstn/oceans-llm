export interface ApiEnvelope<T> {
  data: T
  meta?: {
    generatedAt: string
  }
}

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

export interface UsageCostPoint {
  day: string
  amountUsd: number
}

export interface RequestLogView {
  id: string
  model: string
  provider: string
  statusCode: number
  latencyMs: number
  tokens: number
  timestamp: string
}

export interface TeamView {
  id: string
  name: string
  users: number
  status: 'active' | 'inactive'
}

export interface UserView {
  id: string
  email: string
  role: 'viewer' | 'operator' | 'admin'
  team: string
  status: 'active' | 'invited'
}
