export interface Paginated<T> {
  items: T[]
  page: number
  pageSize: number
  total: number
}

export interface McpOauthConnectionView {
  server_id: string
  server_key: string
  display_name: string
  provider_key: string
  required_scopes: string[]
  granted_scopes: string[]
  status: 'connected' | 'expired' | 'disconnected'
  expires_at: string | null
}

export interface McpOauthStartResponse {
  authorization_url: string
}
