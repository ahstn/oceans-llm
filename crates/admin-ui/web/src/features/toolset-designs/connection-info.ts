import type { McpConnectionInfoPayload } from '@/types/api'

const apiKeySetup = {
  label: 'API key',
  value:
    'Set OCEANS_LLM_API_KEY to a gateway API key before starting the client. Store the raw token, without the Bearer prefix.',
  href: null,
}

const accessNote =
  "This is the gateway's aggregate MCP endpoint. Access grants determine the tools available to the API key. Tool sets do not have separate endpoints."

// Synthetic API response for the isolated preview. Production loads backend-rendered snippets.
export const sampleConnectionInfo: McpConnectionInfoPayload = {
  endpoint: 'https://gateway.example.com/mcp',
  client_configurations: [
    {
      key: 'claude-code',
      label: 'Claude Code',
      model_ids: [],
      setup: [
        { label: 'Configuration', value: '.mcp.json in your project root', href: null },
        apiKeySetup,
        {
          label: 'Docs',
          value: 'https://code.claude.com/docs/en/mcp#environment-variable-expansion-in-mcp-json',
          href: 'https://code.claude.com/docs/en/mcp#environment-variable-expansion-in-mcp-json',
        },
      ],
      blocks: [
        {
          label: 'MCP server configuration',
          filename: '.mcp.json',
          content: JSON.stringify(
            {
              mcpServers: {
                oceans: {
                  type: 'http',
                  url: 'https://gateway.example.com/mcp',
                  headers: { Authorization: 'Bearer ${OCEANS_LLM_API_KEY}' },
                },
              },
            },
            null,
            2,
          ),
        },
      ],
      notes: [
        accessNote,
        'Merge the oceans entry into mcpServers if the project already has an .mcp.json file.',
      ],
    },
    {
      key: 'codex',
      label: 'Codex',
      model_ids: [],
      setup: [
        { label: 'Configuration', value: '~/.codex/config.toml', href: null },
        apiKeySetup,
        {
          label: 'Docs',
          value: 'https://learn.chatgpt.com/docs/extend/mcp?surface=cli',
          href: 'https://learn.chatgpt.com/docs/extend/mcp?surface=cli',
        },
      ],
      blocks: [
        {
          label: 'MCP server configuration',
          filename: 'config.toml',
          content:
            '[mcp_servers.oceans]\nurl = "https://gateway.example.com/mcp"\nbearer_token_env_var = "OCEANS_LLM_API_KEY"\n',
        },
      ],
      notes: [
        accessNote,
        'Merge this section into your config.toml. Replace an existing mcp_servers.oceans section instead of adding it twice.',
      ],
    },
  ],
}
