//! HTTP clients for traffic to upstream MCP servers: discovery refresh and
//! the proxy.
//!
//! A platform admin registers a platform-owned server and may point it
//! anywhere, so its traffic uses a plain client. A team admin may have written
//! a team-owned server's URL, and team admins reach public addresses only.
//! That is checked when the URL is saved, but a name can resolve somewhere
//! else later (DNS rebinding), so a team-owned server's traffic also goes
//! through the outbound fetcher's guarded resolver, which checks every address
//! when it connects. The scope comes from the URL as written
//! ([`OutboundAddressScope::of_trusted_url`]), never from a DNS answer: a
//! hostname reaches public addresses unless it is listed under
//! `mcp.oauth.private_network_hosts`, and an IP-literal host, which was checked
//! when it was saved, keeps its own kind of address. See
//! `docs/adr/2026-10-17-delegated-mcp-server-ownership.md`.

use std::sync::Arc;

use gateway_core::ExternalMcpServerRecord;
use url::Url;

use crate::outbound_fetch::{OutboundAddressScope, guarded_client_builder};

#[derive(Debug, Clone)]
pub struct McpUpstreamClients {
    platform: reqwest::Client,
    /// One client per [`OutboundAddressScope`], in its order.
    team: Arc<[reqwest::Client; 3]>,
    private_network_hosts: Arc<[String]>,
}

impl McpUpstreamClients {
    /// `private_network_hosts` must be normalized, as
    /// [`crate::outbound_fetch::normalize_domains`] returns them.
    #[must_use]
    pub fn new(private_network_hosts: &[String]) -> Self {
        // Timeouts are per request, from each server's own setting.
        let team = |scope| {
            guarded_client_builder(scope)
                .build()
                .expect("guarded MCP HTTP client configuration must be valid")
        };
        Self {
            platform: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("MCP HTTP client configuration must be valid"),
            team: Arc::new([
                team(OutboundAddressScope::Public),
                team(OutboundAddressScope::PrivateNetwork),
                team(OutboundAddressScope::Loopback),
            ]),
            private_network_hosts: private_network_hosts.into(),
        }
    }

    /// The addresses `server`'s traffic may reach, or `None` for a
    /// platform-owned server, whose traffic is not limited. An unparseable URL
    /// gets the public scope.
    #[must_use]
    pub fn scope_for(&self, server: &ExternalMcpServerRecord) -> Option<OutboundAddressScope> {
        server.ownership.owner_team_id?;
        Some(match Url::parse(&server.server_url) {
            Ok(url) => OutboundAddressScope::of_trusted_url(&url, &self.private_network_hosts),
            Err(_) => OutboundAddressScope::Public,
        })
    }

    /// The client for requests to `server`.
    #[must_use]
    pub fn for_server(&self, server: &ExternalMcpServerRecord) -> &reqwest::Client {
        match self.scope_for(server) {
            None => &self.platform,
            Some(OutboundAddressScope::Public) => &self.team[0],
            Some(OutboundAddressScope::PrivateNetwork) => &self.team[1],
            Some(OutboundAddressScope::Loopback) => &self.team[2],
        }
    }
}

impl Default for McpUpstreamClients {
    fn default() -> Self {
        Self::new(&[])
    }
}

#[cfg(test)]
mod tests {
    use gateway_core::{
        ExternalMcpAuthMode, ExternalMcpServerStatus, ExternalMcpTransport, McpServerOwnership,
    };
    use serde_json::Map;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::*;

    fn server(url: &str, owner_team_id: Option<Uuid>) -> ExternalMcpServerRecord {
        let now = OffsetDateTime::now_utc();
        ExternalMcpServerRecord {
            mcp_server_id: Uuid::new_v4(),
            server_key: "notes".to_string(),
            display_name: "Notes".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: url.to_string(),
            auth_mode: ExternalMcpAuthMode::None,
            auth_config: Map::new(),
            timeout_ms: 1_000,
            status: ExternalMcpServerStatus::Active,
            last_discovery_status: None,
            last_discovery_at: None,
            last_successful_discovery_at: None,
            last_error_summary: None,
            last_tool_count: None,
            created_at: now,
            updated_at: now,
            disabled_at: None,
            ownership: match owner_team_id {
                Some(team_id) => McpServerOwnership::team(team_id, None),
                None => McpServerOwnership::default(),
            },
        }
    }

    #[test]
    fn only_team_owned_servers_are_scoped() {
        let clients = McpUpstreamClients::new(&["corp.internal".to_string()]);
        let team = Some(Uuid::new_v4());
        let scope = |url, owner| clients.scope_for(&server(url, owner));

        assert_eq!(scope("http://mcp.corp.internal:8080/mcp", None), None);
        assert_eq!(scope("http://10.0.0.5/mcp", None), None);
        assert_eq!(
            scope("https://mcp.example.com/mcp", team),
            Some(OutboundAddressScope::Public)
        );
        // A name decides nothing by what it resolves to.
        assert_eq!(
            scope("http://localhost.localdomain:8080/mcp", team),
            Some(OutboundAddressScope::Public)
        );
        assert_eq!(
            scope("https://mcp.corp.internal/mcp", team),
            Some(OutboundAddressScope::PrivateNetwork)
        );
        assert_eq!(
            scope("http://10.0.0.5/mcp", team),
            Some(OutboundAddressScope::PrivateNetwork)
        );
        assert_eq!(
            scope("http://127.0.0.1:9000/mcp", team),
            Some(OutboundAddressScope::Loopback)
        );
        assert_eq!(scope("not a url", team), Some(OutboundAddressScope::Public));
    }

    #[tokio::test]
    async fn public_scope_refuses_names_that_resolve_internally() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let router = axum::Router::new().route("/mcp", axum::routing::post(|| async { "ok" }));
            axum::serve(listener, router).await.unwrap();
        });
        let clients = McpUpstreamClients::default();
        let url = format!("http://localhost:{port}/mcp");

        // `localhost` is a name, so it reaches the resolver and resolves to
        // loopback, which the public scope refuses at connect time. A team
        // server registered by a name that later points inside lands here.
        let error = clients.team[0]
            .post(&url)
            .send()
            .await
            .expect_err("the guarded resolver refuses loopback");
        let mut source = std::error::Error::source(&error);
        let mut refused = false;
        while let Some(current) = source {
            refused |= current.to_string().contains("not allowed");
            source = current.source();
        }
        assert!(refused, "unexpected error: {error:?}");

        // Written as the server URL, the same name is loopback by its own
        // spelling, and a platform-owned server is not limited at all.
        let team_server = server(&url, Some(Uuid::new_v4()));
        assert_eq!(
            clients.scope_for(&team_server),
            Some(OutboundAddressScope::Loopback)
        );
        for owner in [Some(Uuid::new_v4()), None] {
            let response = clients
                .for_server(&server(&url, owner))
                .post(&url)
                .send()
                .await
                .expect("reachable");
            assert!(response.status().is_success());
        }
    }
}
