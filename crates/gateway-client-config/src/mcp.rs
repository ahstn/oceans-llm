use serde::Serialize;
use serde_json::json;

use crate::{
    ClientConfig, ClientConfigCodeBlock, ClientConfigSetupItem, DEFAULT_API_KEY_ENV_VAR,
    format::{to_pretty_json, to_pretty_toml},
};

const CLAUDE_CODE_MCP_DOCS_URL: &str =
    "https://code.claude.com/docs/en/mcp#environment-variable-expansion-in-mcp-json";
const CODEX_MCP_DOCS_URL: &str = "https://learn.chatgpt.com/docs/extend/mcp?surface=cli";
const MCP_ACCESS_NOTE: &str = "This is the gateway's aggregate MCP endpoint. Access grants determine the tools available to the API key. Tool sets do not have separate endpoints.";

/// Render MCP connection settings without changing the client's model configuration.
#[must_use]
pub fn render_mcp_client_configs(endpoint: &str) -> Vec<ClientConfig> {
    let claude_code = json!({
        "mcpServers": {
            "oceans": {
                "type": "http",
                "url": endpoint,
                "headers": {
                    "Authorization": format!("Bearer ${{{DEFAULT_API_KEY_ENV_VAR}}}")
                }
            }
        }
    });
    let codex = CodexMcpConfig {
        mcp_servers: CodexMcpServers {
            oceans: CodexMcpServer {
                url: endpoint,
                bearer_token_env_var: DEFAULT_API_KEY_ENV_VAR,
            },
        },
    };

    vec![
        ClientConfig {
            key: "claude-code".to_string(),
            label: "Claude Code".to_string(),
            model_ids: Vec::new(),
            setup: mcp_setup(".mcp.json in your project root", CLAUDE_CODE_MCP_DOCS_URL),
            blocks: vec![ClientConfigCodeBlock {
                label: "MCP server configuration".to_string(),
                filename: ".mcp.json".to_string(),
                content: to_pretty_json(&claude_code),
            }],
            notes: vec![
                MCP_ACCESS_NOTE.to_string(),
                "Merge the oceans entry into mcpServers if the project already has an .mcp.json file."
                    .to_string(),
            ],
        },
        ClientConfig {
            key: "codex".to_string(),
            label: "Codex".to_string(),
            model_ids: Vec::new(),
            setup: mcp_setup("~/.codex/config.toml", CODEX_MCP_DOCS_URL),
            blocks: vec![ClientConfigCodeBlock {
                label: "MCP server configuration".to_string(),
                filename: "config.toml".to_string(),
                content: to_pretty_toml(&codex),
            }],
            notes: vec![
                MCP_ACCESS_NOTE.to_string(),
                "Merge this section into your config.toml. Replace an existing mcp_servers.oceans section instead of adding it twice."
                    .to_string(),
            ],
        },
    ]
}

fn mcp_setup(path: &str, docs_url: &str) -> Vec<ClientConfigSetupItem> {
    vec![
        ClientConfigSetupItem {
            label: "Configuration".to_string(),
            value: path.to_string(),
            href: None,
        },
        ClientConfigSetupItem {
            label: "API key".to_string(),
            value: format!(
                "Set {DEFAULT_API_KEY_ENV_VAR} to a gateway API key before starting the client. Store the raw token, without the Bearer prefix."
            ),
            href: None,
        },
        ClientConfigSetupItem {
            label: "Docs".to_string(),
            value: docs_url.to_string(),
            href: Some(docs_url.to_string()),
        },
    ]
}

#[derive(Serialize)]
struct CodexMcpConfig<'a> {
    mcp_servers: CodexMcpServers<'a>,
}

#[derive(Serialize)]
struct CodexMcpServers<'a> {
    oceans: CodexMcpServer<'a>,
}

#[derive(Serialize)]
struct CodexMcpServer<'a> {
    url: &'a str,
    bearer_token_env_var: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_endpoints_and_authentication_round_trip() {
        for endpoint in [
            "https://gateway.example.com/mcp",
            "https://gateway.example.com/\"quoted\"/back\\slash\n/mcp",
        ] {
            let configs = render_mcp_client_configs(endpoint);
            let claude: serde_json::Value =
                serde_json::from_str(&configs[0].blocks[0].content).unwrap();
            assert_eq!(
                claude,
                json!({
                    "mcpServers": {
                        "oceans": {
                            "type": "http",
                            "url": endpoint,
                            "headers": { "Authorization": "Bearer ${OCEANS_LLM_API_KEY}" }
                        }
                    }
                })
            );

            let codex: toml::Value = toml::from_str(&configs[1].blocks[0].content).unwrap();
            let server = &codex["mcp_servers"]["oceans"];
            assert_eq!(server["url"].as_str(), Some(endpoint));
            assert_eq!(
                server["bearer_token_env_var"].as_str(),
                Some("OCEANS_LLM_API_KEY")
            );
            assert_eq!(codex.as_table().unwrap().len(), 1);
            assert_eq!(codex["mcp_servers"].as_table().unwrap().len(), 1);
            assert_eq!(server.as_table().unwrap().len(), 2);
        }
    }

    #[test]
    fn mcp_setup_has_client_paths_and_credential_guidance() {
        let configs = render_mcp_client_configs("https://gateway.example.com/mcp");
        assert_eq!(configs.len(), 2);
        for (config, key, label, path, filename, docs) in [
            (
                &configs[0],
                "claude-code",
                "Claude Code",
                ".mcp.json in your project root",
                ".mcp.json",
                CLAUDE_CODE_MCP_DOCS_URL,
            ),
            (
                &configs[1],
                "codex",
                "Codex",
                "~/.codex/config.toml",
                "config.toml",
                CODEX_MCP_DOCS_URL,
            ),
        ] {
            assert_eq!(config.key, key);
            assert_eq!(config.label, label);
            assert!(config.model_ids.is_empty());
            assert_eq!(config.blocks.len(), 1);
            assert_eq!(config.blocks[0].filename, filename);
            assert_eq!(config.blocks[0].label, "MCP server configuration");
            assert_eq!(config.setup.len(), 3);
            assert_eq!(config.setup[0].label, "Configuration");
            assert_eq!(config.setup[0].value, path);
            assert_eq!(config.setup[1].label, "API key");
            assert!(config.setup[1].value.contains(DEFAULT_API_KEY_ENV_VAR));
            assert!(config.setup[1].value.contains("without the Bearer prefix"));
            assert_eq!(config.setup[2].label, "Docs");
            assert_eq!(config.setup[2].value, docs);
            assert_eq!(config.setup[2].href.as_deref(), Some(docs));
            assert_eq!(config.notes[0], MCP_ACCESS_NOTE);
        }
    }
}
