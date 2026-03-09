export function requireEnv(name: string): string {
  const value = process.env[name]
  if (!value) {
    throw new Error(`Missing required environment variable: ${name}`)
  }

  return value
}

export function stubAdminUrl(pathname: string): string {
  const upstreamPort = requireEnv('E2E_UPSTREAM_PORT')
  return `http://127.0.0.1:${upstreamPort}${pathname}`
}
