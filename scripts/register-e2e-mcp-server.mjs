// Registers an MCP server in a migrated E2E database before the gateway starts, because
// startup rejects a guardrail override for an MCP server that is not registered.
// Usage: node scripts/register-e2e-mcp-server.mjs <database> <server-key> <server-url>
import { randomUUID } from 'node:crypto'
import { DatabaseSync } from 'node:sqlite'

const [databasePath, serverKey, serverUrl] = process.argv.slice(2)
if (!databasePath || !serverKey || !serverUrl) {
  console.error('usage: register-e2e-mcp-server.mjs <database> <server-key> <server-url>')
  process.exit(2)
}

const now = Math.floor(Date.now() / 1000)
const database = new DatabaseSync(databasePath)
database
  .prepare(
    `INSERT INTO external_mcp_servers (
      mcp_server_id, server_key, display_name, transport, server_url, auth_mode,
      timeout_ms, status, created_at, updated_at
    ) VALUES (?, ?, ?, 'streamable_http', ?, 'none', 30000, 'active', ?, ?)`,
  )
  .run(randomUUID(), serverKey, serverKey, serverUrl, now, now)
database.close()
