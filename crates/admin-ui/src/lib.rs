mod proxy;

pub use proxy::mount_admin_ui;

#[derive(Debug, Clone)]
pub struct AdminUiConfig {
    pub base_path: String,
    pub upstream: String,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
}

impl Default for AdminUiConfig {
    fn default() -> Self {
        Self {
            base_path: "/admin".to_string(),
            upstream: "http://localhost:3001".to_string(),
            connect_timeout_ms: 750,
            request_timeout_ms: 10_000,
        }
    }
}
