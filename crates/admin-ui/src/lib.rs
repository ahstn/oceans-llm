pub mod client_ip;
mod proxy;

pub use proxy::mount_admin_ui;

#[derive(Debug, Clone)]
pub struct AdminUiConfig {
    pub base_path: String,
    pub upstream: String,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
    /// Archive limit plus multipart framing overhead, enforced before proxy buffering.
    pub max_multipart_body_bytes: usize,
    /// Key that signs the browser's address into `x-oceans-client-ip`. Without it the
    /// proxy forwards no client address and audit entries record the direct peer.
    pub client_ip_signing_key: Option<std::sync::Arc<[u8]>>,
}

impl Default for AdminUiConfig {
    fn default() -> Self {
        Self {
            base_path: "/admin".to_string(),
            upstream: "http://localhost:3001".to_string(),
            connect_timeout_ms: 750,
            request_timeout_ms: 10_000,
            max_multipart_body_bytes: 10 * 1024 * 1024 + 64 * 1024,
            client_ip_signing_key: None,
        }
    }
}
